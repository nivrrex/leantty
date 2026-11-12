# Probe14: real hwndFocus + hide console window + focus tty window + SendInput
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WE {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO lpgui);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
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
    [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
        public int cbSize; public int flags; public IntPtr hwndActive; public IntPtr hwndFocus;
        public IntPtr hwndCapture; public IntPtr hwndMenuOwner; public IntPtr hwndMoveSize;
        public IntPtr hwndCaret; public int rcLeft; public int rcTop; public int rcRight; public int rcBottom;
    }
}
'@

[void][WE]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WE+INPUT))

$env:APP_TRACE_FILE = "$OUT\probe14.trace"
$env:APP_DUMP_SNAPSHOT = "$OUT\probe14.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:APP_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe14.err"
$Script:TargetPid = [uint64]$p.Id
"pid=$Script:TargetPid"
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WE+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WE]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        $tt = New-Object System.Text.StringBuilder 256
        [void][WE]::GetClassName($h, $cls, 256)
        [void][WE]::GetWindowText($h, $tt, 256)
        $r = New-Object WE+RECT
        [void][WE]::GetWindowRect($h, [ref]$r)
        $pidOut2 = 0
        $tid = [WE]::GetWindowThreadProcessId($h, [ref]$pidOut2)
        $Script:Wins += [PSCustomObject]@{ hwnd = $h; tid = $tid; cls = $cls.ToString(); title = $tt.ToString(); vis = [WE]::IsWindowVisible($h); w = $r.right - $r.left; h = $r.bottom - $r.top }
    }
    return $true
}
[void][WE]::EnumWindows($cb, [IntPtr]::Zero)
$real = $Script:Wins | Where-Object { $_.cls -eq 'Window Class' } | Select-Object -First 1
$console = $Script:Wins | Where-Object { $_.cls -eq 'ConsoleWindowClass' } | Select-Object -First 1
"real=$($real.hwnd) console=$($console.hwnd)"

# raw focus state BEFORE any intervention
$tmp = 0
$mainTid = [WE]::GetWindowThreadProcessId($real.hwnd, [ref]$tmp)
$gui = New-Object WE+GUITHREADINFO
$gui.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($gui)
[void][WE]::GetGUIThreadInfo($mainTid, [ref]$gui)
"raw: active=$($gui.hwndActive) focus=$($gui.hwndFocus) focusIsReal=$($gui.hwndFocus -eq $real.hwnd) focusIsConsole=$($gui.hwndFocus -eq $console.hwnd)"

# hide console window, then focus real window
if ($console -ne $null) { [void][WE]::ShowWindow($console.hwnd, 0) }
[void][WE]::ShowWindow($real.hwnd, 9)
$myTid = [WE]::GetCurrentThreadId()
if ($real.tid -ne $myTid) { [void][WE]::AttachThreadInput($myTid, $real.tid, $true) }
[void][WE]::SetFocus($real.hwnd)
$fr = [WE]::SetForegroundWindow($real.hwnd)
if ($real.tid -ne $myTid) { [void][WE]::AttachThreadInput($myTid, $real.tid, $false) }
Start-Sleep -Milliseconds 400
[void][WE]::GetGUIThreadInfo($mainTid, [ref]$gui)
"after-focus: setFg=$fr focus=$($gui.hwndFocus) focusIsReal=$($gui.hwndFocus -eq $real.hwnd) fg=$([WE]::GetForegroundWindow())"

function Send-VkE([uint32]$vk) {
    $n = 2
    $inputs = New-Object WE+INPUT[] $n
    $inputs[0] = New-Object WE+INPUT
    $inputs[0].type = 1; $inputs[0].u.ki.wVk = [uint16]$vk; $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object WE+INPUT
    $inputs[1].type = 1; $inputs[1].u.ki.wVk = [uint16]$vk; $inputs[1].u.ki.dwFlags = 0x0002
    [uint32][WE]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}
foreach ($ch in 'echo hello'.ToCharArray()) {
    $vk = [uint32][byte][char]::ToUpper($ch)
    if ($ch -eq ' ') { $vk = 0x20 }
    $s = Send-VkE $vk
    "vk=0x{0:X} sent=$s" -f $vk
    Start-Sleep -Milliseconds 60
}
[uint32](Send-VkE 0x0D) | Out-Null
Start-Sleep -Seconds 5
"keydbg-lines=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue).Count)"
"--- KEYDBG ---"
Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue | Select-Object -First 8 | ForEach-Object { $_.Line }
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 5
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
