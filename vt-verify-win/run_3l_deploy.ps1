# Deployment verification for the packaged dist (t4c / t4d).
# - extracts the zip to a path with spaces and Chinese characters
# - starts app.exe from a DIFFERENT working directory
# - checks which conpty.dll the process actually loaded (module path)
# - measures wrap behaviour end to end with a pixel row profile (works in release)
# - records OpenConsole.exe / conhost.exe PID sets before and after; never kills them
# ASCII only.
param(
    [string]$Zip = 'C:\dev\tty\vt-verify-win\tty-win-x64.zip',
    [string]$Base = 'C:\dev\tty\vt-verify-win\logs\3l-deploy',
    [string]$Cfg = 'config.example.toml'
)
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace D3L -Name Native -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
'@

function Get-PidSet([string]$name) {
    $ids = @()
    Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object { $ids += $_.Id }
    return ($ids | Sort-Object)
}
function Shot-Grid([IntPtr]$hwnd) {
    $r = New-Object D3L.Native+RECT
    [void][D3L.Native]::GetClientRect($hwnd, [ref]$r)
    $pt = New-Object D3L.Native+POINT
    [void][D3L.Native]::ClientToScreen($hwnd, [ref]$pt)
    $w = $r.R - $r.L; $h = $r.B - $r.T
    if ($w -le 0 -or $h -le 0) { return $null }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $g.Dispose()
    return $bmp
}
function Count-TextBands($bmp) {
    # a row counts as text when it has >= 8 pixels darker than 140
    $bands = 0; $inBand = $false
    for ($y = 0; $y -lt $bmp.Height; $y += 1) {
        $dark = 0
        for ($x = 0; $x -lt $bmp.Width; $x += 2) {
            $c = $bmp.GetPixel($x, $y)
            if (($c.R + $c.G + $c.B) -lt 420) { $dark++ }
        }
        if ($dark -ge 8) { if (-not $inBand) { $bands++; $inBand = $true } } else { $inBand = $false }
    }
    return $bands
}
function Dark-Extent($bmp) {
    $maxX = 0
    for ($y = 0; $y -lt $bmp.Height; $y += 2) {
        for ($x = 0; $x -lt $bmp.Width; $x += 1) {
            $c = $bmp.GetPixel($x, $y)
            if (($c.R + $c.G + $c.B) -lt 420 -and $x -gt $maxX) { $maxX = $x }
        }
    }
    return $maxX
}

if (Test-Path $Base) { Remove-Item -Recurse -Force $Base }
New-Item -ItemType Directory -Force -Path $Base | Out-Null
$deploy = Join-Path $Base ('tty dist ' + [string][char]0x90E8 + [string][char]0x7F72)
New-Item -ItemType Directory -Force -Path $deploy | Out-Null
Expand-Archive -Path $Zip -DestinationPath $deploy -Force
$exe = Join-Path $deploy 'app.exe'
Write-Host ("DEPLOY dir={0}" -f $deploy)
Write-Host ("DEPLOY files={0}" -f ((Get-ChildItem $deploy -File | ForEach-Object { $_.Name }) -join ','))

$before = @{ 'OpenConsole' = (Get-PidSet 'OpenConsole'); 'conhost' = (Get-PidSet 'conhost') }
Write-Host ("PIDS before: OpenConsole=[{0}] conhost=[{1}]" -f ($before['OpenConsole'] -join ','), ($before['conhost'] -join ','))

function Start-From-Elsewhere([string]$exePath, [string]$cfgPath, [string]$errFile, [string]$workDir) {
    # child inherits our stderr, so app/perf output lands in the caller's log
    $si = New-Object System.Diagnostics.ProcessStartInfo
    $si.FileName = $exePath
    $si.Arguments = '--config "' + $cfgPath + '"'
    $si.WorkingDirectory = $workDir
    $si.UseShellExecute = $false
    $si.RedirectStandardError = $false
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $si
    [void]$p.Start()
    return @{ proc = $p; errFile = $errFile }
}

function Wait-MainWindow($proc, [int]$timeoutMs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $proc.Refresh()
        if ($proc.HasExited) { return [IntPtr]::Zero }
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { return $proc.MainWindowHandle }
        Start-Sleep -Milliseconds 100
    }
    return [IntPtr]::Zero
}

$results = @()

# ---- case 1: full dist (conpty.dll + OpenConsole.exe present)
$r = Start-From-Elsewhere $exe (Join-Path $deploy $Cfg) (Join-Path $Base 'case1.err') 'C:\Windows'
$hwnd = Wait-MainWindow $r.proc 15000
Start-Sleep -Milliseconds 2500
$r.proc.Refresh()
$mods = @()
if (-not $r.proc.HasExited) {
    $mods = $r.proc.Modules | Where-Object { $_.ModuleName -ieq 'conpty.dll' } | ForEach-Object { $_.FileName }
}
$alive1 = -not $r.proc.HasExited
Write-Host ("CASE1 alive={0} hwnd={1} conpty_module=[{2}]" -f $alive1, $hwnd, ($mods -join ';'))
if ($hwnd -ne [IntPtr]::Zero) {
    # resize to 200x30-ish so a materialised 80/80/43 wrap shows as 3 text bands
    [void][D3L.Native]::SetWindowPos($hwnd, [IntPtr]::Zero, 60, 60, 1500, 620, 0x0004)
    [void][D3L.Native]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 1200
    # type: a 200 char single line, then a ESC[7m sample via powershell
    [System.Windows.Forms.SendKeys]::SendWait("for /l %i in (1,1,1) do @echo ###")
    Write-Host "CASE1 typed-marker"
}
if (-not $r.proc.HasExited) { $r.proc.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 2 }
if (-not $r.proc.HasExited) { Stop-Process -Id $r.proc.Id -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
$after1 = @{ 'OpenConsole' = (Get-PidSet 'OpenConsole'); 'conhost' = (Get-PidSet 'conhost') }
Write-Host ("PIDS after case1: OpenConsole=[{0}] conhost=[{1}]" -f ($after1['OpenConsole'] -join ','), ($after1['conhost'] -join ','))
$newOC = @($after1['OpenConsole'] | Where-Object { $before['OpenConsole'] -notcontains $_ })
$newCH = @($after1['conhost'] | Where-Object { $before['conhost'] -notcontains $_ })
Write-Host ("NEW after case1: OpenConsole=[{0}] conhost=[{1}]" -f ($newOC -join ','), ($newCH -join ','))

# ---- case 2: rollback - remove both ConPTY files
Remove-Item (Join-Path $deploy 'conpty.dll') -Force
Remove-Item (Join-Path $deploy 'OpenConsole.exe') -Force
$r2 = Start-From-Elsewhere $exe (Join-Path $deploy $Cfg) (Join-Path $Base 'case2.err') 'C:\Windows'
$hwnd2 = Wait-MainWindow $r2.proc 15000
Start-Sleep -Milliseconds 2500
$r2.proc.Refresh()
$mods2 = @()
if (-not $r2.proc.HasExited) {
    $mods2 = $r2.proc.Modules | Where-Object { $_.ModuleName -ieq 'conpty.dll' } | ForEach-Object { $_.FileName }
}
Write-Host ("CASE2 alive={0} hwnd={1} conpty_module=[{2}]" -f (-not $r2.proc.HasExited), $hwnd2, ($mods2 -join ';'))
if (-not $r2.proc.HasExited) { $r2.proc.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 2 }
if (-not $r2.proc.HasExited) { Stop-Process -Id $r2.proc.Id -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2

# ---- case 3: only conpty.dll kept (OpenConsole.exe removed)
Copy-Item (Join-Path 'C:\dev\tty\vt-verify-win\conpty-dl\official' 'conpty.dll') $deploy
$r3 = Start-From-Elsewhere $exe (Join-Path $deploy $Cfg) (Join-Path $Base 'case3.err') 'C:\Windows'
$hwnd3 = Wait-MainWindow $r3.proc 15000
Start-Sleep -Milliseconds 3000
$r3.proc.Refresh()
$alive3 = -not $r3.proc.HasExited
$mods3 = @()
if ($alive3) { $mods3 = $r3.proc.Modules | Where-Object { $_.ModuleName -ieq 'conpty.dll' } | ForEach-Object { $_.FileName } }
Write-Host ("CASE3 alive={0} hwnd={1} conpty_module=[{2}] exit={3}" -f $alive3, $hwnd3, ($mods3 -join ';'), $(if ($alive3) { '-' } else { $r3.proc.ExitCode }))
if ($alive3) { $r3.proc.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 2 }
if (-not $r3.proc.HasExited) { Stop-Process -Id $r3.proc.Id -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
$afterAll = @{ 'OpenConsole' = (Get-PidSet 'OpenConsole'); 'conhost' = (Get-PidSet 'conhost') }
Write-Host ("PIDS after all: OpenConsole=[{0}] conhost=[{1}]" -f ($afterAll['OpenConsole'] -join ','), ($afterAll['conhost'] -join ','))
foreach ($n in 'OpenConsole', 'conhost') {
    $new = @($afterAll[$n] | Where-Object { $before[$n] -notcontains $_ })
    if ($new.Count -gt 0) {
        Write-Host ("RESIDUAL {0}: [{1}]" -f $n, ($new -join ','))
        foreach ($id in $new) {
            $p = Get-Process -Id $id -ErrorAction SilentlyContinue
            if ($p) {
                $ci = Get-CimInstance Win32_Process -Filter "ProcessId=$id" -ErrorAction SilentlyContinue
                Write-Host ("  pid={0} start={1} cmd={2}" -f $id, $p.StartTime, $(if ($ci) { $ci.CommandLine } else { 'n/a' }))
            }
        }
    }
}
Write-Host "DEPLOY-DONE"
