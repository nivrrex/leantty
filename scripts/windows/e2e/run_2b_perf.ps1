# Windows 2B e2e part 3: release startup latency.
# Measures [perf] first_frame_ms from stderr for 5 cold-ish starts per config,
# plus a "type immediately" run to check that early input is not swallowed
# while the background font load is still running.
param([string]$AppBin = '')
. (Join-Path $PSScriptRoot 'wb2_lib.ps1') -AppBin $AppBin
$CFG = Join-Path $ROOT 'scripts\windows\e2e\cfg'

function Measure-Start([string]$name, [string]$cfgPath, [int]$runs) {
    $times = @()
    for ($i = 1; $i -le $runs; $i++) {
        $dump = Join-Path $OUT "$name-$i.dump"
        $err = Join-Path $OUT "$name-$i.err"
        Remove-Item $dump, $err -ErrorAction SilentlyContinue
        $env:APP_DUMP_SNAPSHOT = $dump
        # -ArgumentList with an empty array is rejected by Start-Process
        if ($cfgPath) {
            $p = Start-Process -FilePath $AppBin -ArgumentList @('--config', $cfgPath) -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        } else {
            $p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
        }
        $ms = -1
        $t0 = Get-Date
        while (((Get-Date) - $t0).TotalSeconds -lt 60) {
            if (Test-Path $err) {
                $t = Get-Content $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
                if ($t -and ($t -match 'first_frame_ms=(\d+)')) { $ms = [int]$Matches[1]; break }
            }
            Start-Sleep -Milliseconds 100
        }
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Milliseconds 400
        $times += $ms
    }
    $valid = $times | Where-Object { $_ -ge 0 } | Sort-Object
    $median = -1; $spread = -1
    if ($valid.Count -gt 0) {
        $median = $valid[[int][math]::Floor($valid.Count / 2)]
        $spread = $valid[$valid.Count - 1] - $valid[0]
    }
    @{ times = $times; median = $median; spread = $spread }
}

# the dump hook only exists in debug builds, so the early-input case runs the
# debug binary while the latency numbers above come from the release binary
function Measure-FirstInput([string]$name, [string]$cfgPath) {
    $Script:AppBin = Join-Path $ROOT 'target\debug\app.exe'
    $a = Start-App2 $name $cfgPath
    $Script:AppBin = $AppBin
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ ok = $false; detail = 'no window' } }
    $null = Focus-App2 $a.hwnd
    # type immediately, before the background font load can have finished
    Send-Text2 'echo earlyinput'; Send-Vk2 0x0D
    $ok = Wait-Dump2 $a.dump 'earlyinput' 45
    $grid = (Dump-Grid $a.dump).grid
    $ran = ($grid | Where-Object { $_.Trim() -eq 'earlyinput' }).Count -gt 0
    Stop-App2 $a.proc
    @{ ok = ($ok -and $ran); detail = "echoed=$ok executed=$ran" }
}

$d = Measure-Start 'perf-default' $null 5
Report2 't4-default-startup' ($d.median -ge 0) "(times=($($d.times -join ',')) median=$($d.median)ms spread=$($d.spread)ms)"

$w = Measure-Start 'perf-windowstoml' (Join-Path $CFG 'ps-win.toml') 5
Report2 't4-windowstoml-startup' ($w.median -ge 0) "(times=($($w.times -join ',')) median=$($w.median)ms spread=$($w.spread)ms)"

$f = Measure-FirstInput 't4-first-input' (Join-Path $CFG 'clip-win.toml')
Report2 't4-early-input' $f.ok "($($f.detail))"

Write-Host "==== 2B perf summary: PASS=$PASS FAIL=$FAIL ===="
if ($FAIL -gt 0) { exit 1 }
