# t2: GPU failure paths on a machine without a real GPU adapter.
# Every launch uses the 3N direct-handle wrapper (cmd /c "... 2>log") with
# APP_PERF_FRAMES=1; nothing here uses PowerShell's Start-Process redirection.
# Only the app.exe started by this script is ever stopped (and it is expected to
# exit by itself for backend=gpu).
# ASCII only.
param(
    [string]$Kit = 'C:\dev\tty\vt-verify-win\3o-gpu',
    [string]$App = 'C:\dev\tty\target\release\app.exe'
)
$ErrorActionPreference = 'Stop'
$dir = $Kit
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force -Path $dir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $dir 'logs') | Out-Null

# minimal configs derived from the kit templates (no absolute paths inside)
$templates = 'C:\dev\tty\scripts\windows\tonight\configs'
foreach ($n in 'cpu', 'gpu', 'gpu-auto') {
    Copy-Item (Join-Path $templates ($n + '.toml')) (Join-Path $dir ($n + '.toml'))
}

function Run-Case {
    param([string]$Tag, [string]$Cfg, [hashtable]$EnvVars = @{}, [int]$WaitExitSec = 120, [switch]$ExpectExit)
    $log = Join-Path $dir ('logs\' + $Tag + '.log')
    $wrapper = Join-Path $dir ('run-' + $Tag + '.cmd')
    $lines = @('@echo off', ('cd /d "{0}"' -f $dir), 'set APP_PERF_FRAMES=1')
    foreach ($k in $EnvVars.Keys) { $lines += ('set {0}={1}' -f $k, $EnvVars[$k]) }
    $lines += ('"{0}" --config "{1}" 2>"{2}"' -f $App, (Join-Path $dir $Cfg), $log)
    $lines += 'echo EXITCODE=%ERRORLEVEL%'
    $lines -join "`r`n" | Set-Content -Path $wrapper -Encoding Default

    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ('"{0}"' -f $wrapper)) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $dir ('logs\' + $Tag + '.out'))
    # backend=gpu exits by itself within seconds; backend=auto keeps running with a
    # window, so wait for exit only up to $WaitExitSec and then inspect it instead
    $exited = $p.WaitForExit($WaitExitSec * 1000)
    $sw.Stop()
    if (-not $exited) {
        if (-not $ExpectExit) {
            Write-Host ("T2 {0} still running after {1}s (expected for this case)" -f $Tag, $WaitExitSec)
        } else {
            Write-Host ("T2 {0} HUNG after {1}s (still running pid={2})" -f $Tag, $WaitExitSec, $p.Id)
        }
    }
    $out = ''
    $o = Join-Path $dir ('logs\' + $Tag + '.out')
    if (Test-Path $o) { $out = (Get-Content $o -Raw -Encoding UTF8) }
    $code = 'n/a'
    if ($out -match 'EXITCODE=(-?\d+)') { $code = $Matches[1] }
    $text = ''
    if (Test-Path $log) { $text = Get-Content $log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
    $errLines = @()
    if ($text) {
        foreach ($l in ($text -split "`r?`n")) {
            if ($l -match 'ERROR|error|panic|failed|不可用|adapter|适配器|gpu_adapters|gpu_init_ms') { $errLines += $l }
        }
    }
    Write-Host ("T2 {0} cmd_exit={1} seconds={2:N1} log_bytes={3}" -f $Tag, $code, $sw.Elapsed.TotalSeconds, $(if (Test-Path $log) { (Get-Item $log).Length } else { 0 }))
    $i = 0
    foreach ($l in $errLines) {
        if ($i -ge 8) { Write-Host '   ...'; break }
        Write-Host ('   | ' + $l)
        $i++
    }
    $appPid = 0
    try {
        $child = Get-CimInstance Win32_Process -Filter "Name='app.exe'" -ErrorAction SilentlyContinue |
            Where-Object { [int]$_.ParentProcessId -eq $p.Id } | Select-Object -First 1
        if ($child) { $appPid = [int]$child.ProcessId }
    } catch { }
    if ($appPid -ne 0) {
        $still = Get-Process -Id $appPid -ErrorAction SilentlyContinue
        Write-Host ('T2 {0} app child pid={1} still_alive={2}' -f $Tag, $appPid, ($null -ne $still))
        if ($still) {
            $appProc = Get-Process -Id $appPid -ErrorAction SilentlyContinue
            if ($appProc) {
                $h = $appProc.MainWindowHandle
                Write-Host ('T2 {0} app window={1} ws_mb={2}' -f $Tag, $h, [math]::Round($appProc.WorkingSet64 / 1MB))
                try { [void]$appProc.CloseMainWindow() } catch { }
                Start-Sleep -Seconds 2
                $appProc.Refresh()
                if (-not $appProc.HasExited) { try { Stop-Process -Id $appPid -ErrorAction SilentlyContinue } catch { } }
            }
        }
    }
}

Write-Host 'T2 == backend=gpu (default env) =='
Run-Case 'gpu-plain' 'gpu.toml' -ExpectExit

Write-Host 'T2 == backend=gpu + WGPU_BACKEND=dx12 =='
Run-Case 'gpu-dx12' 'gpu.toml' @{ WGPU_BACKEND = 'dx12' } -ExpectExit

Write-Host 'T2 == backend=gpu + WGPU_BACKEND=vulkan =='
Run-Case 'gpu-vulkan' 'gpu.toml' @{ WGPU_BACKEND = 'vulkan' } -ExpectExit

Write-Host 'T2 == backend=auto (must stay on CPU) =='
Run-Case 'auto' 'gpu-auto.toml' -WaitExitSec 8

Write-Host 'T2-DONE'
