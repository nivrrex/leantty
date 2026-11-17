# t3f: paste delivery and loop_gap on the WARP path.
# 1 MB / 5 MB exact delivery through the real path (clipboard via .NET, right-click
# paste), 5 MB interrupted by Ctrl-C, and the loop_gap maximum with ping running.
# ASCII only.
param(
    [string]$Dir = 'C:\dev\tty\vt-verify-win\3q-warp',
    [string]$App = 'C:\dev\tty\target\release\leantty.exe',
    [string]$Probe = 'C:\dev\tty\vt-verify-win\probe_paste\target\release\pp.exe'
)
$ErrorActionPreference = 'Stop'
. 'C:\dev\tty\scripts\windows\tonight\lib.ps1'
Add-Type -AssemblyName System.Windows.Forms
$gpuCfg = Join-Path $Dir 'gpu.toml'
$warpEnv = @{ LEANTTY_GPU_ALLOW_SOFTWARE = '1'; WGPU_BACKEND = 'dx12'; WGPU_ADAPTER_NAME = 'Basic' }
$rows = @()

function Start-Warp {
    $log = Join-Path $Dir ('logs\paste-' + [Guid]::NewGuid().ToString('N').Substring(0, 6) + '.log')
    $h = Start-AppDirect -Exe $App -Config $gpuCfg -LogPath $log -WorkDir $Dir -EnvVars $warpEnv
    if (-not $h.App) { throw 'warp app failed to start' }
    $hwnd = Wait-AppWindow -App $h.App -TimeoutMs 30000
    if ($hwnd -eq [IntPtr]::Zero) { throw 'warp window missing' }
    [void](Focus-Window $hwnd)
    Start-Sleep -Milliseconds 900
    return [pscustomobject]@{ Handle = $h; Hwnd = $hwnd; Log = $log }
}

function Paste-Run([int]$mb, [string]$tag) {
    $out = Join-Path $Dir ('probe-' + $tag + '.txt')
    if (Test-Path $out) { Remove-Item $out -Force }
    $env:PROBE_OUT = $out
    $s = Start-Warp
    try {
        Send-Line ('"' + $Probe + '"')
        Start-Sleep -Seconds 3
        $text = 'a' * ($mb * 1024 * 1024)
        [System.Windows.Forms.Clipboard]::SetText($text)
        Start-Sleep -Milliseconds 500
        $g = Get-Geometry $s.Hwnd
        $x = [int]($g.x + $g.w / 2); $y = [int]($g.y + $g.h / 2)
        Click-Button $x $y 0x0002 0x0004     # focus the window
        Start-Sleep -Milliseconds 200
        Click-Button $x $y 0x0008 0x0010     # right-click -> Paste binding
        $want = $mb * 1024 * 1024
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $seen = -1
        while ($sw.Elapsed.TotalSeconds -lt 180) {
            Start-Sleep -Milliseconds 500
            if (Test-Path $out) {
                $t = Get-Content $out -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
                if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1]; if ($seen -ge $want) { break } }
            }
        }
        Write-Host ('T3F paste{0}MB bytes={1} expect={2} seconds={3:N1}' -f $mb, $seen, $want, $sw.Elapsed.TotalSeconds)
        $script:rows += [pscustomobject]@{ case = ('paste' + $mb + 'mb'); bytes = $seen; expect = $want }
    } finally {
        Stop-OwnApp $s.Handle.App $s.Handle.Cmd
        Start-Sleep -Seconds 1
    }
}

function CtrlC-Run {
    $out = Join-Path $Dir 'probe-ctrlc.txt'
    if (Test-Path $out) { Remove-Item $out -Force }
    $env:PROBE_OUT = $out
    $s = Start-Warp
    try {
        Send-Line ('"' + $Probe + '"')
        Start-Sleep -Seconds 3
        [System.Windows.Forms.Clipboard]::SetText('a' * (5 * 1024 * 1024))
        Start-Sleep -Milliseconds 500
        $g = Get-Geometry $s.Hwnd
        $x = [int]($g.x + $g.w / 2); $y = [int]($g.y + $g.h / 2)
        Click-Button $x $y 0x0002 0x0004
        Start-Sleep -Milliseconds 150
        Click-Button $x $y 0x0008 0x0010
        Set-Mod 0x11 $true
        Send-Vk2 0x43
        Set-Mod 0x11 $false
        Start-Sleep -Seconds 8
        $seen = -1
        if (Test-Path $out) {
            $t = Get-Content $out -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -match 'BYTES=(\d+)') { $seen = [int]$Matches[1] }
        }
        Write-Host ('T3F paste5MB+ctrlC bytes={0}' -f $seen)
        $script:rows += [pscustomobject]@{ case = 'paste5mb_ctrlc'; bytes = $seen; expect = -1 }
    } finally {
        Stop-OwnApp $s.Handle.App $s.Handle.Cmd
        Start-Sleep -Seconds 1
    }
}

function LoopGap-Run {
    $s = Start-Warp
    try {
        Send-Line 'ping -t 127.0.0.1'
        Start-Sleep -Seconds 3
        [System.Windows.Forms.Clipboard]::SetText('a' * (5 * 1024 * 1024))
        Start-Sleep -Milliseconds 300
        $g = Get-Geometry $s.Hwnd
        $x = [int]($g.x + $g.w / 2); $y = [int]($g.y + $g.h / 2)
        Click-Button $x $y 0x0002 0x0004
        Start-Sleep -Milliseconds 150
        $sw = [Diagnostics.Stopwatch]::StartNew()
        Click-Button $x $y 0x0008 0x0010
        while ($sw.Elapsed.TotalSeconds -lt 8) { [void][TN.Win]::SetCursorPos($x + 3, $y + 3); Start-Sleep -Milliseconds 40 }
        $all = @(Get-Content $s.Log -Encoding UTF8 | Where-Object { $_ -match 'loop_gap_ms=(\d+)' })
        $max = 0
        foreach ($l in $all) { if ($l -match 'loop_gap_ms=(\d+)') { $v = [int]$Matches[1]; if ($v -gt $max) { $max = $v } } }
        Write-Host ('T3F loop_gap_max_ms={0} samples={1}' -f $max, $all.Count)
        $script:rows += [pscustomobject]@{ case = 'loop_gap'; bytes = $max; expect = 100 }
    } finally {
        Stop-OwnApp $s.Handle.App $s.Handle.Cmd
        Start-Sleep -Seconds 1
    }
}

Paste-Run 1 '1mb'
Paste-Run 5 '5mb'
CtrlC-Run
LoopGap-Run
$rows | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Dir 't3f-paste.json') -Encoding UTF8
Write-Host 'T3F-DONE'
