# Probe11: screenshot desktop + window styles + child windows; keys to every window
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WB {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr hWnd, EnumChildProc lpfn, IntPtr lParam);
    public delegate bool EnumChildProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left; public int top; public int right; public int bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
        public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT {
        public uint uMsg; public ushort wParamL; public ushort wParamH;
    }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION {
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public HARDWAREINPUT hi;
    }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT {
        public uint type; public INPUTUNION u;
    }
}
'@

[void][WB]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WB+INPUT))

$env:APP_TRACE_FILE = "$OUT\probe11.trace"
$env:APP_DUMP_SNAPSHOT = "$OUT\probe11.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:APP_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe11.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"main-hwnd=$hwnd"
$rect = New-Object WB+RECT
[void][WB]::GetWindowRect($hwnd, [ref]$rect)
"rect=$($rect.left),$($rect.top)-$($rect.right),$($rect.bottom) vis=$([WB]::IsWindowVisible($hwnd))"
$cls = New-Object System.Text.StringBuilder 256; $tt = New-Object System.Text.StringBuilder 256
[void][WB]::GetClassName($hwnd, $cls, 256); [void][WB]::GetWindowText($hwnd, $tt, 256)
"cls=$($cls.ToString()) title='$($tt.ToString())' style=0x$([WB]::GetWindowLong($hwnd, -16).ToString('X8')) ex=0x$([WB]::GetWindowLong($hwnd, -20).ToString('X8'))"
$kids = @()
$cb = [WB+EnumChildProc]{ param([IntPtr]$h, [IntPtr]$l) $Script:kids += $h; return $true }
[void][WB]::EnumChildWindows($hwnd, $cb, [IntPtr]::Zero)
"child-count=$($kids.Count)"

# screenshot full screen
$bmp = New-Object System.Drawing.Bitmap([System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width, [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$bmp.Save("$OUT\probe11.png", [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"screenshot saved"

# trace size before keys
"trace-before=$(@(Get-Content $env:APP_TRACE_FILE -ErrorAction SilentlyContinue).Count)"

# focus + SendInput 'echo hi' + Enter
$tmp = 0
$appTid = [WB]::GetWindowThreadProcessId($hwnd, [ref]$tmp)
$myTid = [WB]::GetCurrentThreadId()
[void][WB]::ShowWindow($hwnd, 9)
[WB]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [WB]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
if ($appTid -ne $myTid) { [void][WB]::AttachThreadInput($myTid, $appTid, $true) }
[void][WB]::SetFocus($hwnd)
[void][WB]::SetForegroundWindow($hwnd)
if ($appTid -ne $myTid) { [void][WB]::AttachThreadInput($myTid, $appTid, $false) }
"focused fg=$([WB]::GetForegroundWindow()) match=$([WB]::GetForegroundWindow() -eq $hwnd)"

function Send-VkB([uint32]$vk) {
    $n = 2
    $inputs = New-Object WB+INPUT[] $n
    $inputs[0] = New-Object WB+INPUT
    $inputs[0].type = 1; $inputs[0].u.ki.wVk = [uint16]$vk; $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object WB+INPUT
    $inputs[1].type = 1; $inputs[1].u.ki.wVk = [uint16]$vk; $inputs[1].u.ki.dwFlags = 0x0002
    [uint32][WB]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}
foreach ($ch in 'echo hi'.ToCharArray()) {
    $vk = [uint32][byte][char]::ToUpper($ch)
    if ($ch -eq ' ') { $vk = 0x20 }
    [void](Send-VkB $vk)
    Start-Sleep -Milliseconds 60
}
[void](Send-VkB 0x0D)
Start-Sleep -Seconds 5
"trace-after=$(@(Get-Content $env:APP_TRACE_FILE -ErrorAction SilentlyContinue).Count)"
"--- trace tail ---"
Get-Content $env:APP_TRACE_FILE -ErrorAction SilentlyContinue | Select-Object -Last 6
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
