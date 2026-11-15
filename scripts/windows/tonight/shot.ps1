# shot.ps1 - DPI-aware helper. Runs in its OWN process on purpose:
# Windows PowerShell 5.1 has already fixed the process DPI awareness by the time a
# script runs (a later SetProcessDpiAwarenessContext returns false with
# ERROR_ACCESS_DENIED), so awareness must be set at the very first statement of a
# fresh process, before any assembly or window is created.
# ASCII only.
param(
    [ValidateSet('info', 'shot', 'size')]
    [string]$Mode = 'info',
    [string]$Out = '',
    [string]$Hwnd = '0'
)
# --- first statement: per-monitor-v2 (falls back to v1 / system aware) ----------
$aware = $false
try {
    $aware = [ShotDpi]::SetProcessDpiAwarenessContext([IntPtr](-4))
} catch {
    try {
        Add-Type -Namespace ShotDpi2 -Name W -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
        $aware = [ShotDpi2.W]::SetProcessDPIAware()
    } catch { $aware = $false }
}
Add-Type -Namespace ShotDpi -Name W -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[StructLayout(LayoutKind.Sequential)] public struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }
public delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdc, ref RECT r, IntPtr data);
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
[DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc cb, IntPtr data);
[DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr hm, ref MONITORINFO mi);
[DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hm, int type, out uint x, out uint y);
'@

function Get-Displays {
    $script:mon = @()
    $cb = [ShotDpi.W+MonitorEnumProc]{
        param($hm, $hdc, [ref]$r, $data)
        $mi = New-Object ShotDpi.W+MONITORINFO
        $mi.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($mi)
        $dx = 0; $dy = 0
        [void][ShotDpi.W]::GetDpiForMonitor($hm, 0, [ref]$dx, [ref]$dy)
        $name = ''
        if ([ShotDpi.W]::GetMonitorInfo($hm, [ref]$mi)) {
            $name = ('{0}x{1}@({2},{3})' -f ($mi.rcMonitor.R - $mi.rcMonitor.L), ($mi.rcMonitor.B - $mi.rcMonitor.T), $mi.rcMonitor.L, $mi.rcMonitor.T)
        }
        $script:mon += , ([pscustomobject]@{
            monitor = $name
            dpiX = [int]$dx
            dpiY = [int]$dy
            scale = if ($dx -gt 0) { [math]::Round($dx / 96.0, 3) } else { 0 }
        })
        return $true
    }
    [void][ShotDpi.W]::EnumDisplayMonitors([IntPtr]::Zero, [IntPtr]::Zero, $cb, [IntPtr]::Zero)
    return $script:mon
}

if ($Mode -eq 'info') {
    $d = @(Get-Displays)
    $o = [pscustomobject]@{ aware = $aware; displays = $d; pid = $PID }
    $o | ConvertTo-Json -Depth 5 -Compress
    exit 0
}

$h = [IntPtr][int64]$Hwnd
if ($h -eq [IntPtr]::Zero) { Write-Host '{"error":"no hwnd"}'; exit 2 }
$c = New-Object ShotDpi.W+RECT
if (-not [ShotDpi.W]::GetClientRect($h, [ref]$c)) { Write-Host '{"error":"GetClientRect"}'; exit 2 }
$w = $c.R - $c.L
$hh = $c.B - $c.T
$pt = New-Object ShotDpi.W+POINT
[void][ShotDpi.W]::ClientToScreen($h, [ref]$pt)

if ($Mode -eq 'size') {
    $o = [pscustomobject]@{ aware = $aware; client_w = $w; client_h = $hh; screen_x = $pt.X; screen_y = $pt.Y }
    $o | ConvertTo-Json -Compress
    exit 0
}

if ($Mode -eq 'shot') {
    if ($w -le 0 -or $hh -le 0) { Write-Host '{"error":"client area empty"}'; exit 2 }
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size $w, $hh))
    $g.Dispose()
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $o = [pscustomobject]@{ aware = $aware; client_w = $w; client_h = $hh; bytes = (Get-Item $Out).Length; path = $Out }
    $o | ConvertTo-Json -Compress
    exit 0
}
Write-Host '{"error":"unknown mode"}'
exit 2
