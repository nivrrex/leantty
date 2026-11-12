# Probe8: enumerate ALL windows of the app thread + PostMessage WM_CHAR to each; find which hwnd is winit's
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class W9 {
    [DllImport("user32.dll")] public static extern bool EnumThreadWindows(uint dwThreadId, EnumThreadProc lpfn, IntPtr lParam);
    public delegate bool EnumThreadProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO lpgui);
    [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
        public int cbSize; public int flags; public IntPtr hwndActive; public IntPtr hwndFocus;
        public IntPtr hwndCapture; public IntPtr hwndMenuOwner; public IntPtr hwndMoveSize;
        public IntPtr hwndCaret; public int rcLeft; public int rcTop; public int rcRight; public int rcBottom;
    }
}
'@

$Script:Windows = @()
$EnumCb = [W9+EnumThreadProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $cls = New-Object System.Text.StringBuilder 256
    [void][W9]::GetClassName($h, $cls, 256)
    $txt = New-Object System.Text.StringBuilder 256
    [void][W9]::GetWindowText($h, $txt, 256)
    $Script:Windows += [PSCustomObject]@{ hwnd = $h; cls = $cls.ToString(); txt = $txt.ToString(); vis = [W9]::IsWindowVisible($h) }
    return $true
}

$env:APP_DUMP_SNAPSHOT = "$OUT\probe8.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe8.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"main-hwnd=$hwnd"
$tmp = 0
$tid = [W9]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
"app-tid=$tid"
[void][W9]::EnumThreadWindows($tid, $EnumCb, [IntPtr]::Zero)
"--- windows ---"
$Script:Windows | ForEach-Object { "hwnd=$($_.hwnd) cls=$($_.cls) vis=$($_.vis) txt='$($_.txt)'" }

# focus main window, then PostMessage WM_CHAR 'q' to EVERY window
[void][W9]::ShowWindow($hwnd, 9)
$appTid = $tid
$myTid = [W9]::GetCurrentThreadId()
if ($appTid -ne $myTid) { [void][W9]::AttachThreadInput($myTid, $appTid, $true) }
[void][W9]::SetFocus($hwnd)
[void][W9]::SetForegroundWindow($hwnd)
if ($appTid -ne $myTid) { [void][W9]::AttachThreadInput($myTid, $appTid, $false) }
$gui = New-Object W9+GUITHREADINFO
$gui.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($gui)
[void][W9]::GetGUIThreadInfo($tid, [ref]$gui)
"focus-window=$($gui.hwndFocus) match-main=$($gui.hwndFocus -eq $hwnd)"

foreach ($w in $Script:Windows) {
    [void][W9]::PostMessage($w.hwnd, 0x0102, [IntPtr]([uint16][char]'q'), [IntPtr]1)
    "posted WM_CHAR q -> $($w.hwnd) ($($w.cls))"
}
Start-Sleep -Seconds 5
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
