param([int]$ProcId, [string]$Text, [string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class KT {
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] p, int cb);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL, wParamH; }
  [StructLayout(LayoutKind.Explicit)] public struct IU { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public HARDWAREINPUT hi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public IU u; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[void][KT]::SetProcessDPIAware()
$p = Get-Process -Id $ProcId -ErrorAction SilentlyContinue
if (-not $p) { Write-Output "no proc"; exit 2 }
$h = [IntPtr]::Zero
for ($i=0; $i -lt 40; $i++) { $p.Refresh(); if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }; Start-Sleep -Milliseconds 500 }
if ($h -eq [IntPtr]::Zero) { Write-Output "no hwnd"; exit 3 }
[void][KT]::ShowWindow($h, 9)
$tmp = 0
$fg = [KT]::GetForegroundWindow()
$fgTid = [KT]::GetWindowThreadProcessId($fg, [ref]$tmp)
$myTid = [KT]::GetCurrentThreadId()
[void][KT]::AttachThreadInput($myTid, $fgTid, $true)
[void][KT]::SetForegroundWindow($h)
[void][KT]::AttachThreadInput($myTid, $fgTid, $false)
Start-Sleep -Milliseconds 500
Write-Output ("fg=" + [KT]::GetForegroundWindow() + " target=" + $h)
# 逐字符 KEYEVENTF_UNICODE
$Text = $Text + [char]13
foreach ($ch in $Text.ToCharArray()) {
  $scan = [ushort][int][char]$ch
  $in = New-Object KT+INPUT[] 2
  $in[0] = New-Object KT+INPUT; $in[0].type = 1; $in[0].u.ki.wScan = $scan; $in[0].u.ki.dwFlags = 4
  $in[1] = New-Object KT+INPUT; $in[1].type = 1; $in[1].u.ki.wScan = $scan; $in[1].u.ki.dwFlags = 6
  [void][KT]::SendInput(2, $in, [System.Runtime.InteropServices.Marshal]::SizeOf([KT+INPUT]))
}
Start-Sleep -Seconds 2
$r = New-Object KT+RECT; [void][KT]::GetClientRect($h, [ref]$r)
$pt = New-Object KT+POINT; [void][KT]::ClientToScreen($h, [ref]$pt)
$bmp = New-Object System.Drawing.Bitmap ($r.Right-$r.Left), ($r.Bottom-$r.Top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc(); [void][KT]::PrintWindow($h, $hdc, 2); $g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Write-Output ("saved " + $Out)
