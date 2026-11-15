# t5b: functional check of the PTY without synthetic typing.
# The config in each test dist is pointed at a shell that writes a marker file and
# then lingers, so "did the app's PTY actually run the child's command" is proven
# by the file, not by keystroke injection (which could not be validated here).
# ASCII only.
param(
    [string]$Base = 'C:\dev\tty\vt-verify-win\logs\3m-deploy',
    [int]$LingerSec = 20
)
$ErrorActionPreference = 'Stop'

function Set-CfgShell([string]$dist, [string]$markerName) {
    $cfgPath = Join-Path $dist 'config.example.toml'
    $lines = Get-Content $cfgPath -Encoding UTF8
    $out = @()
    foreach ($l in $lines) {
        if ($l -match '^shell\s*=') {
            $out += ('shell = {{ program = "cmd.exe", args = ["/c", "echo MARK> ' + $markerName + ' & timeout /t ' + $LingerSec + ' >nul"] }}')
        } else {
            $out += $l
        }
    }
    $out -join "`r`n" | Set-Content -Path $cfgPath -Encoding UTF8
}

function Start-App([string]$exe, [string]$cfg, [string]$workDir) {
    $si = New-Object System.Diagnostics.ProcessStartInfo
    $si.FileName = $exe
    $si.Arguments = '--config "' + $cfg + '"'
    $si.WorkingDirectory = $workDir
    $si.UseShellExecute = $false
    $si.RedirectStandardError = $false
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $si
    [void]$p.Start()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 20000) {
        $p.Refresh()
        if ($p.HasExited) { return $null }
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { return $p }
        Start-Sleep -Milliseconds 150
    }
    return $null
}

function Run([string]$label, [string]$dist, [string]$markerName) {
    $marker = Join-Path $dist $markerName
    if (Test-Path $marker) { Remove-Item $marker -Force }
    Set-CfgShell $dist $markerName
    $p = Start-App (Join-Path $dist 'app.exe') (Join-Path $dist 'config.example.toml') $dist
    if (-not $p) { Write-Host ("T5B {0} START-FAILED" -f $label); return }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $ok = $false
    while ($sw.Elapsed.TotalSeconds -lt 15) {
        if (Test-Path $marker) { $ok = $true; break }
        Start-Sleep -Milliseconds 200
    }
    $took = [int]$sw.Elapsed.TotalMilliseconds
    $children = @()
    foreach ($c in (Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)) {
        if ([int]$c.ParentProcessId -eq $p.Id) { $children += $c }
    }
    Write-Host ("T5B {0} pid={1} alive={2} marker_in_15s={3} took_ms={4} marker_len={5}" -f $label, $p.Id, (-not $p.HasExited), $ok, $took, $(if ($ok) { (Get-Item $marker).Length } else { -1 }))
    foreach ($c in $children) { Write-Host ("T5B {0} child name={1} pid={2} cmd={3}" -f $label, $c.Name, $c.ProcessId, $c.CommandLine) }
    $p.CloseMainWindow() | Out-Null
    Start-Sleep -Seconds 2
    $p.Refresh()
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 1
}

$full = Join-Path $Base 'dist full'
$dllOnly = Join-Path $Base 'dist dll only'
$before = @()
Get-Process OpenConsole -ErrorAction SilentlyContinue | ForEach-Object { $before += $_.Id }
Write-Host ("T5B openconsole_before=[{0}]" -f ($before -join ','))
Run 'full' $full 'm2.txt'
Run 'dllonly' $dllOnly 'm2.txt'
Start-Sleep -Seconds 2
$after = @()
Get-Process OpenConsole -ErrorAction SilentlyContinue | ForEach-Object { $after += $_.Id }
$new = @($after | Where-Object { $before -notcontains $_ })
Write-Host ("T5B openconsole_after=[{0}] new=[{1}]" -f ($after -join ','), ($new -join ','))
foreach ($id in $new) {
    $pr = Get-Process -Id $id -ErrorAction SilentlyContinue
    $ci = Get-CimInstance Win32_Process -Filter "ProcessId=$id" -ErrorAction SilentlyContinue
    if ($pr) { Write-Host ("T5B residual openconsole pid={0} start={1} cmd={2}" -f $id, $pr.StartTime, $(if ($ci) { $ci.CommandLine } else { 'n/a' })) }
}
Write-Host "T5B-DONE"
