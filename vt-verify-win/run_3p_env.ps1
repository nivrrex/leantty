# t2: how does WGPU_BACKEND actually reach app.exe?
# Five launch variants, all with backend="gpu" (so the app exits 1 on this
# GPU-less machine) and APP_PERF_FRAMES=1, stderr to a direct file handle.
# The interesting value is `backends=` on the gpu_instance line.
# ASCII only.
param(
    [string]$Dir = 'C:\dev\tty\vt-verify-win\3p-env',
    [string]$App = 'C:\dev\tty\target\release\app.exe',
    [string]$CfgSrc = 'C:\dev\tty\scripts\windows\tonight\configs\gpu.toml'
)
$ErrorActionPreference = 'Stop'
if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dir 'logs') | Out-Null
Copy-Item $CfgSrc (Join-Path $Dir 'gpu.toml')

function Show-Backends([string]$Tag) {
    $log = Join-Path $Dir ('logs\' + $Tag + '.log')
    $line = $null
    if (Test-Path $log) {
        foreach ($l in (Get-Content $log -Encoding UTF8 -ErrorAction SilentlyContinue)) {
            if ($l -match 'gpu_instance_ms') { $line = $l }
        }
    }
    if ($line) {
        if ($line -match 'backends=([^ ]+)') { Write-Host ('T2 {0} backends={1}' -f $Tag, $Matches[1]) }
        else { Write-Host ('T2 {0} line={1}' -f $Tag, $line) }
    } else {
        Write-Host ('T2 {0} no gpu_instance line (log={1} bytes)' -f $Tag, $(if (Test-Path $log) { (Get-Item $log).Length } else { 0 }))
    }
}

function Run-Wrapper([string]$Tag, [string[]]$Sets, [string]$SetStyle) {
    # $Sets: list of "NAME=VALUE" pairs; $SetStyle: 'plain' | 'nospace' | 'quoted'
    $wrapper = Join-Path $Dir ('w-' + $Tag + '.cmd')
    $lines = @('@echo off', ('cd /d "{0}"' -f $Dir), 'set APP_PERF_FRAMES=1')
    foreach ($kv in $Sets) {
        switch ($SetStyle) {
            'plain'   { $lines += ('set {0} && echo set-ok' -f $kv) }
            'nospace' { $lines += ('set {0}&& echo set-ok' -f $kv) }
            'quoted'  { $lines += ('set "{0}"' -f $kv) }
        }
    }
    $lines += ('"{0}" --config "{1}" 2>"{2}"' -f $App, (Join-Path $Dir 'gpu.toml'), (Join-Path $Dir ('logs\' + $Tag + '.log')))
    $lines += 'echo EXITCODE=%ERRORLEVEL%'
    $lines -join "`r`n" | Set-Content -Path $wrapper -Encoding Default
    Write-Host ('T2 {0} WRAPPER-BYTES:' -f $Tag)
    foreach ($l in (Get-Content $wrapper)) { Write-Host ('   ' + $l) }
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ('"{0}"' -f $wrapper)) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Dir ('logs\' + $Tag + '.out'))
    [void]$p.WaitForExit(60000)
    if (Test-Path (Join-Path $Dir ('logs\' + $Tag + '.out'))) {
        $o = Get-Content (Join-Path $Dir ('logs\' + $Tag + '.out')) -Raw
        Write-Host ('T2 {0} cmd_out={1}' -f $Tag, ($o -replace "`r?`n", ' '))
    }
    Show-Backends $Tag
}

Write-Host '== 1) set NAME=value && app  (space before &&) =='
Run-Wrapper 'plain-dx12' @('WGPU_BACKEND=dx12') 'plain'

Write-Host '== 2) set NAME=value&& app  (no space, still plain) =='
Run-Wrapper 'nospace-dx12' @('WGPU_BACKEND=dx12') 'nospace'

Write-Host '== 3) set "NAME=value"  (quoted, canonical) =='
Run-Wrapper 'quoted-vulkan' @('WGPU_BACKEND=vulkan') 'quoted'

Write-Host '== 4) PowerShell process env + Start-Process (no wrapper) =='
$env:WGPU_BACKEND = 'dx12'
try {
    $p = Start-Process -FilePath $App -ArgumentList @('--config', (Join-Path $Dir 'gpu.toml')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $Dir 'logs\psenv-dx12.log')
    [void]$p.WaitForExit(60000)
} finally {
    Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue
}
Show-Backends 'psenv-dx12'

Write-Host '== 5) the kit launcher (lib.ps1 Start-AppDirect) with Env set =='
. 'C:\dev\tty\scripts\windows\tonight\lib.ps1'
$env:WGPU_BACKEND = 'vulkan'
try {
    $h = Start-AppDirect -Exe $App -Config (Join-Path $Dir 'gpu.toml') -LogPath (Join-Path $Dir 'logs\kit-vulkan.log') -WorkDir $Dir
    if ($h.App) { [void]$h.App.WaitForExit(60000) }
    Start-Sleep -Milliseconds 500
    Write-Host ('T2 kit-vulkan WRAPPER-RAW:')
    if ($h.Wrapper -and (Test-Path $h.Wrapper)) { foreach ($l in (Get-Content $h.Wrapper)) { Write-Host ('   ' + $l) } }
    Stop-OwnApp $h.App $h.Cmd
} finally {
    Remove-Item Env:WGPU_BACKEND -ErrorAction SilentlyContinue
}
Show-Backends 'kit-vulkan'

Write-Host 'T2-DONE'
