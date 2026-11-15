# t2: WGPU_BACKEND parsing on Windows (4Q). Nine values, backend="gpu",
# APP_PERF_FRAMES=1, direct file handle via a wrapper .cmd (set "K=V" form).
# Records backends_env / backends_effective, adapter enumeration, exit code, time
# and the error text. No software-adapter switch here on purpose.
# ASCII only.
param(
    [string]$Dir = 'C:\dev\tty\vt-verify-win\3q-backend',
    [string]$App = 'C:\dev\tty\target\release\app.exe'
)
$ErrorActionPreference = 'Stop'
if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dir 'logs') | Out-Null
$cfg = Join-Path $Dir 'gpu.toml'
$src = Get-Content 'C:\dev\tty\scripts\windows\tonight\configs\gpu.toml' -Raw -Encoding UTF8
$src | Set-Content $cfg -Encoding UTF8

$cases = @(
    @{ tag = 'dx12'; value = 'dx12' },
    @{ tag = 'DX12'; value = 'DX12' },
    @{ tag = 'dx12-space'; value = 'dx12 ' },
    @{ tag = 'vulkan'; value = 'vulkan' },
    @{ tag = 'both'; value = 'dx12,vulkan' },
    @{ tag = 'vk'; value = 'vk' },
    @{ tag = 'dx'; value = 'dx' },
    @{ tag = 'empty'; value = '' },
    @{ tag = 'garbage'; value = 'not-a-backend' }
)

$rows = @()
foreach ($c in $cases) {
    $wrapper = Join-Path $Dir ('w-' + $c.tag + '.cmd')
    $log = Join-Path $Dir ('logs\' + $c.tag + '.log')
    $setLine = if ($c.value -eq '') { 'set "WGPU_BACKEND="' } else { ('set "WGPU_BACKEND={0}"' -f $c.value) }
    @(
        '@echo off',
        ('cd /d "{0}"' -f $Dir),
        'set "APP_PERF_FRAMES=1"',
        'set "APP_GPU_ALLOW_SOFTWARE="',
        $setLine,
        ('"{0}" --config "{1}" 2>"{2}"' -f $App, $cfg, $log),
        'echo EXITCODE=%ERRORLEVEL%'
    ) -join "`r`n" | Set-Content -Path $wrapper -Encoding Default

    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/c', ('"{0}"' -f $wrapper)) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Dir ('logs\' + $c.tag + '.out'))
    # backend=gpu must exit by itself; give it 30 s and record a hang if it does not
    $exited = $p.WaitForExit(30000)
    $sw.Stop()
    if (-not $exited) {
        try { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue } catch { }
    }
    $code = 'HUNG'
    $outFile = Join-Path $Dir ('logs\' + $c.tag + '.out')
    if (Test-Path $outFile) {
        $o = Get-Content $outFile -Raw
        if ($o -match 'EXITCODE=(-?\d+)') { $code = $Matches[1] }
    }
    $txt = ''
    if (Test-Path $log) { $txt = Get-Content $log -Raw -Encoding UTF8 -ErrorAction SilentlyContinue }
    $envRaw = ''
    $eff = ''
    if ($txt -match "backends_env='([^']*)'") { $envRaw = $Matches[1] }
    if ($txt -match 'backends_effective=([^ ]+) count=(\d+)') { $eff = $Matches[1] + ' count=' + $Matches[2] }
    $adapters = @()
    foreach ($line in ($txt -split "`r?`n")) { if ($line -match 'gpu_adapters') { $adapters += $line } }
    $err = ''
    foreach ($line in ($txt -split "`r?`n")) {
        if ($line -match '^Error:|backend error|适配器枚举为空|无法识别|Backend\(') { $err = $line.Trim(); break }
    }
    $rows += [pscustomobject]@{
        case = $c.tag; set_value = $c.value; exit = $code; seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
        backends_env = $envRaw; backends_effective = $eff; adapters = $adapters.Count; err = $err
    }
    Write-Host ('T2 {0,-11} exit={1,-6} {2,5:N2}s env=''{3}'' eff={4} adapters={5}' -f $c.tag, $code, $sw.Elapsed.TotalSeconds, $envRaw, $eff, $adapters.Count)
    if ($err) { Write-Host ('   err=' + $err) }
    foreach ($a in $adapters) { Write-Host ('   | ' + $a) }
}
$rows | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Dir 't2-backends.json') -Encoding UTF8
Write-Host 'T2-DONE'
