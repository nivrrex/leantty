# Task 3 (minimal, repeated): app inside a fresh console host window, then a real
# Ctrl+C sent to that host window. Tracks the app PID that this case started.
param([string]$AppBin = '')
$ROOT = Split-Path -Parent $PSScriptRoot
. (Join-Path $ROOT 'scripts\windows\e2e\wb2_lib.ps1') -AppBin $AppBin
$OUT = Join-Path $ROOT 'vt-verify-win\logs\ctrl-host'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null

function Find-ConsoleByTitle([string]$tag) {
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 250
        foreach ($p in Get-Process -Name 'cmd', 'powershell', 'pwsh' -ErrorAction SilentlyContinue) {
            $h = $p.MainWindowHandle
            if ($h -ne [IntPtr]::Zero) {
                $sb = New-Object System.Text.StringBuilder 512
                [void][WB2]::GetWindowText($h, $sb, 512)
                if ($sb.ToString() -like "*$tag*") { return $h }
            }
        }
    }
    return [IntPtr]::Zero
}

function Run-Case([string]$hostKind, [int]$runNo) {
    $prior = @(Get-Process -Name 'app' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    $tag = "tty-cc-$hostKind-$runNo"
    if ($hostKind -eq 'cmd') {
        $cmdLine = '/c start "' + $tag + '" cmd.exe /k ""' + $AppBin + '""'
    } else {
        $cmdLine = '/c start "' + $tag + '" powershell.exe -NoProfile -Command "& ''' + $AppBin + '''"'
    }
    $host = Start-Process -FilePath 'cmd.exe' -ArgumentList $cmdLine -PassThru -WindowStyle Hidden
    $hwnd = Find-ConsoleByTitle $tag
    if ($hwnd -eq [IntPtr]::Zero) {
        if (-not $host.HasExited) { Stop-Process -Id $host.Id -Force -ErrorAction SilentlyContinue }
        return "run${runNo} host-window-not-found"
    }
    Start-Sleep -Seconds 3
    $appNow = @(Get-Process -Name 'app' -ErrorAction SilentlyContinue |
        Where-Object { $prior -notcontains $_.Id } | ForEach-Object { $_.Id })
    if ($appNow.Count -eq 0) {
        if (-not $host.HasExited) { Stop-Process -Id $host.Id -Force -ErrorAction SilentlyContinue }
        return "run${runNo} app-not-started"
    }
    $null = Focus-App2 $hwnd
    Start-Sleep -Milliseconds 300
    Set-Mod 0x11 $true
    Send-Vk2 0x43
    Set-Mod 0x11 $false
    Start-Sleep -Seconds 3
    $after = @(Get-Process -Name 'app' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    $died = @($appNow | Where-Object { $after -notcontains $_ })
    $survived = @($appNow | Where-Object { $after -contains $_ })
    foreach ($id in $survived) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    if (-not $host.HasExited) { Stop-Process -Id $host.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
    "run${runNo} app=$($appNow -join ',') died=$($died -join ',') survived=$($survived -join ',')"
}

for ($i = 1; $i -le 3; $i++) { Write-Host ("CMD-HOST " + (Run-Case 'cmd' $i)); Start-Sleep -Seconds 1 }
for ($i = 1; $i -le 3; $i++) { Write-Host ("PS-HOST  " + (Run-Case 'ps' $i)); Start-Sleep -Seconds 1 }
