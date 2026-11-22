param([int]$ProcId, [string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WS {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[void][WS]::SetProcessDPIAware()
$p = Get-Process -Id $ProcId -ErrorAction SilentlyContinue
if (-not $p) { Write-Output "no process $ProcId"; exit 2 }
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60; $i++) {
  $p.Refresh()
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }
  Start-Sleep -Milliseconds 500
}
if ($h -eq [IntPtr]::Zero) { Write-Output "no window for $ProcId"; exit 3 }
[void][WS]::ShowWindow($h, 9)
[void][WS]::MoveWindow($h, 40, 40, 1100, 820, $true)
[void][WS]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 1500
$r = New-Object WS+RECT
[void][WS]::GetClientRect($h, [ref]$r)
$pt = New-Object WS+POINT
[void][WS]::ClientToScreen($h, [ref]$pt)
$w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
# PrintWindow：即便窗口不在前台/被遮挡也能拿到本窗口内容（后台抓图，避免前台锁干扰）。
# 窗口刚出现时可能还没画完，若抓到近乎纯色则重试几次。
$bmp = $null
for ($attempt = 0; $attempt -lt 6; $attempt++) {
  if ($bmp) { $bmp.Dispose() }
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  $ok = [WS]::PrintWindow($h, $hdc, 2)
  $g.ReleaseHdc($hdc)
  $g.Dispose()
  # 采样判断是否"有内容"：取 16x16 网格的颜色种类数
  $colors = @{}
  for ($yy = 0; $yy -lt $hh; $yy += [Math]::Max(1, [int]($hh / 16))) {
    for ($xx = 0; $xx -lt $w; $xx += [Math]::Max(1, [int]($w / 16))) {
      $colors[$bmp.GetPixel($xx, $yy).ToArgb()] = 1
    }
  }
  Write-Output ("printwindow={0} attempt={1} colors={2}" -f $ok, $attempt, $colors.Count)
  if ($colors.Count -gt 2) { break }
  Start-Sleep -Milliseconds 800
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output ("saved {0} hwnd={1} client={2}x{3} at {4},{5}" -f $Out, $h, $w, $hh, $pt.X, $pt.Y)
