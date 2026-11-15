# End-to-end discriminator: does the app's PTY materialise wraps?
# A 200-char single line is printed by the shell. With the system conhost the wrap
# is materialised into 80/80/43 columns (3D/3K); with conpty.dll >= 1.25 the line
# reaches the VT unbroken and wraps at the window width (120 columns).
# Observable: furthest dark pixel on the first text row -- ~80 chars vs ~120 chars
# wide -- so no OCR and no debug build is needed.
# ASCII only.
param(
    [string]$Dist = '',
    [string]$Cfg = '',
    [string]$Tag = 'case1',
    [string]$OutDir = 'C:\dev\tty\vt-verify-win\logs\3l-deploy'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -Namespace D3L3 -Name N -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
'@

function Get-ClientSize([IntPtr]$hwnd) {
    $r = New-Object D3L3.N+RECT
    if (-not [D3L3.N]::GetClientRect($hwnd, [ref]$r)) { return @(0, 0) }
    return @(($r.R - $r.L), ($r.B - $r.T))
}

function Start-App([string]$exe, [string]$cfg) {
    $si = New-Object System.Diagnostics.ProcessStartInfo
    $si.FileName = $exe
    $si.Arguments = '--config "' + $cfg + '"'
    $si.WorkingDirectory = 'C:\Windows'
    $si.UseShellExecute = $false
    $si.RedirectStandardError = $false
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $si
    [void]$p.Start()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 25000) {
        $p.Refresh()
        if ($p.HasExited) { return $null }
        $h = $p.MainWindowHandle
        if ($h -ne [IntPtr]::Zero) {
            $sz = Get-ClientSize $h
            if ($sz[0] -gt 200 -and $sz[1] -gt 100) { return $p }
        }
        Start-Sleep -Milliseconds 150
    }
    return $null
}

function Measure-Rows([IntPtr]$hwnd) {
    $sz = Get-ClientSize $hwnd
    $w = $sz[0]; $h = $sz[1]
    if ($w -le 0 -or $h -le 0) { throw "client size invalid: ${w}x${h}" }
    $pt = New-Object D3L3.N+POINT
    [void][D3L3.N]::ClientToScreen($hwnd, [ref]$pt)
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $g.Dispose()
    $bands = @()
    $inBand = $false; $bandStart = 0; $bandMaxX = 0; $bandChars = 0
    for ($y = 0; $y -lt $h; $y++) {
        $dark = 0; $maxX = -1
        for ($x = 0; $x -lt $w; $x += 2) {
            $c = $bmp.GetPixel($x, $y)
            if (($c.R + $c.G + $c.B) -lt 420) { $dark++; if ($x -gt $maxX) { $maxX = $x } }
        }
        $isText = $dark -ge 6
        if ($isText -and -not $inBand) { $inBand = $true; $bandStart = $y; $bandMaxX = $maxX }
        elseif ($isText -and $inBand) { if ($maxX -gt $bandMaxX) { $bandMaxX = $maxX } }
        elseif (-not $isText -and $inBand) {
            $inBand = $false
            $bands += [pscustomobject]@{ y0 = $bandStart; y1 = $y - 1; maxX = $bandMaxX }
        }
    }
    if ($inBand) { $bands += [pscustomobject]@{ y0 = $bandStart; y1 = $h - 1; maxX = $bandMaxX } }
    $bmp.Save((Join-Path $OutDir ("shot-{0}.png" -f $Tag)))
    $bmp.Dispose()
    return $bands
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$p = Start-App (Join-Path $Dist 'app.exe') $Cfg
if (-not $p) { Write-Host ("WRAP {0} FAILED-TO-START" -f $Tag); exit 3 }
Write-Host ("WRAP {0} pid={1} hwnd={2} client={3}" -f $Tag, $p.Id, $p.MainWindowHandle, ((Get-ClientSize $p.MainWindowHandle) -join 'x'))
[void][D3L3.N]::SetForegroundWindow($p.MainWindowHandle)
Start-Sleep -Seconds 3
# the app injects its own shell init, so the 200-char line is typed into the
# running shell instead of being passed as shell args
# reuse the proven input path from the e2e harness (activation dance + retry)
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'scripts\windows\e2e\wb2_lib.ps1') -AppBin (Join-Path $Dist 'app.exe')
$fg = Focus-App2 $p.MainWindowHandle
Write-Host ("WRAP {0} focus={1}" -f $Tag, $fg)
Start-Sleep -Milliseconds 500
for ($k = 1; $k -le 3; $k++) {
    Send-Text2 ('echo ' + ('A' * 200))
    Start-Sleep -Milliseconds 250
    Send-Vk2 0x0D
    Start-Sleep -Seconds 2
    $probe = Measure-Rows $p.MainWindowHandle
    $widest = ($probe | Measure-Object -Property maxX -Maximum).Maximum
    Write-Host ("WRAP {0} try{1} widest_x={2}" -f $Tag, $k, $widest)
    if ($widest -gt 700) { break }
}
$bands = Measure-Rows $p.MainWindowHandle
Write-Host ("WRAP {0} bands={1}" -f $Tag, (($bands | ForEach-Object { "{0}-{1}:x_max={2}" -f $_.y0, $_.y1, $_.maxX }) -join ' | '))
$p.CloseMainWindow() | Out-Null
Start-Sleep -Seconds 2
if (-not $p.HasExited) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
