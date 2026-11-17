# Bisect: mirror run_all.ps1 Start-Case exactly (function + hashtable return) with diagnostics.
$ErrorActionPreference = 'Continue'
$ROOT = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$AppBin = Join-Path $ROOT 'target\debug\leantty.exe'
$OUT = Join-Path $PSScriptRoot 'out'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
Write-Host "ROOT=$ROOT AppBin=$AppBin exists=$(Test-Path $AppBin)"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class Win {
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@

function Find-AppWindow([uint32]$pid, [int]$timeoutSec = 40) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $timeoutSec) {
        $hwnd = [Win]::FindWindow($null, 'tty')
        if ($hwnd -ne [IntPtr]::Zero) {
            $wpid = 0
            [void][Win]::GetWindowThreadProcessId($hwnd, [ref]$wpid)
            if ($wpid -eq $pid) { return $hwnd }
        }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}

function Start-Case([string]$name, [string]$shell, [string]$configPath) {
    $dump = Join-Path $OUT "$name.dump"
    $err = Join-Path $OUT "$name.err"
    Remove-Item $dump -ErrorAction SilentlyContinue
    $env:LEANTTY_DUMP_SNAPSHOT = $dump
    if ($shell) { $env:SHELL = $shell } else { Remove-Item Env:SHELL -ErrorAction SilentlyContinue }
    $argList = @()
    if ($configPath) { $argList = @('--config', $configPath) }
    $p = Start-Process -FilePath $AppBin -ArgumentList $argList -WorkingDirectory $ROOT -PassThru -RedirectStandardError $err
    $hwnd = Find-AppWindow ([uint32]$p.Id)
    if ($hwnd -eq [IntPtr]::Zero) {
        Write-Host "FAIL $name (window not found; pid=$($p.Id))"
        $script:FAIL++
        return @{ ok = $false; pid = $p.Id; hwnd = $hwnd; dump = $dump; err = $err; proc = $p }
    }
    return @{ ok = $true; pid = $p.Id; hwnd = $hwnd; dump = $dump; err = $err; proc = $p }
}

$probe = Start-Case 'fgprobe' 'cmd.exe' $null
Write-Host "probe type: $($probe.GetType().FullName)"
Write-Host "probe count: $($probe.Count)"
if ($probe -is [System.Collections.Hashtable]) {
    Write-Host "ok=$($probe.ok) pid=$($probe.pid) hwnd=$($probe.hwnd) dump=$($probe.dump)"
} else {
    Write-Host "probe is array; elements:"
    $probe | ForEach-Object { Write-Host "  elem type=$($_.GetType().Name) val=$_" }
}
Start-Sleep -Seconds 5
Write-Host "dump exists: $(Test-Path (Join-Path $OUT 'fgprobe.dump'))"
Write-Host "err exists: $(Test-Path (Join-Path $OUT 'fgprobe.err'))"
if ($probe.ok -and $probe.proc -and -not $probe.proc.HasExited) {
    [void][Win]::PostMessage($probe.hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not $probe.proc.WaitForExit(5000)) { $probe.proc.Kill() }
    Write-Host "app closed cleanly"
}
