# Probe12: EnumWindows by pid - find the REAL app window (any thread), focus IT, send keys
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WC {
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
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
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

[void][WC]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WC+INPUT))

$env:APP_TRACE_FILE = "$OUT\probe12.trace"
$env:APP_DUMP_SNAPSHOT = "$OUT\probe12.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:APP_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe12.err"
$targetPid = $p.Id
"pid=$targetPid"
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WC+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WC]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ($pidOut -eq $l) {
        $cls = New-Object System.Text.StringBuilder 256
        $tt = New-Object System.Text.StringBuilder 256
        [void][WC]::GetClassName($h, $cls, 256)
        [void][WC]::GetWindowText($h, $tt, 256)
        $r = New-Object WC+RECT
        [void][WC]::GetWindowRect($h, [ref]$r)
        $tid = [WC]::GetWindowThreadProcessId($h, [ref]$pidOut)
        $Script:Wins += [PSCustomObject]@{ hwnd = $h; tid = $tid; cls = $cls.ToString(); title = $tt.ToString(); vis = [WC]::IsWindowVisible($h); w = $r.right - $r.left; h = $r.bottom - $r.top }
    }
    return $true
}
[void][WC]::EnumWindows($cb, [IntPtr]$targetPid)
"--- all windows of pid ---"
$Script:Wins | ForEach-Object { "hwnd=$($_.hwnd) tid=$($_.tid) w=$($_.w) h=$($_.h) vis=$($_.vis) cls=$($_.cls) title='$($_.title)'" }

$real = $Script:Wins | Where-Object { $_.w -gt 100 -and $_.h -gt 100 } | Select-Object -First 1
if ($null -eq $real) { $real = $Script:Wins | Where-Object { $_.title -ne '' } | Select-Object -First 1 }
"real-hwnd=$($real.hwnd)"

# focus the REAL window with the proven recipe
[void][WC]::ShowWindow($real.hwnd, 9)
[WC]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [WC]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
$myTid = [WC]::GetCurrentThreadId()
if ($real.tid -ne $myTid) { [void][WC]::AttachThreadInput($myTid, $real.tid, $true) }
[void][WC]::SetFocus($real.hwnd)
$r = [WC]::SetForegroundWindow($real.hwnd)
if ($real.tid -ne $myTid) { [void][WC]::AttachThreadInput($myTid, $real.tid, $false) }
"setFg=$r"
Start-Sleep -Milliseconds 400

function Send-VkC([uint32]$vk) {
    $n = 2
    $inputs = New-Object WC+INPUT[] $n
    $inputs[0] = New-Object WC+INPUT
    $inputs[0].type = 1; $inputs[0].u.ki.wVk = [uint16]$vk; $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object WC+INPUT
    $inputs[1].type = 1; $inputs[1].u.ki.wVk = [uint16]$vk; $inputs[1].u.ki.dwFlags = 0x0002
    [uint32][WC]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}
foreach ($ch in 'echo hello'.ToCharArray()) {
    $vk = [uint32][byte][char]::ToUpper($ch)
    if ($ch -eq ' ') { $vk = 0x20 }
    [void](Send-VkC $vk)
    Start-Sleep -Milliseconds 60
}
[void](Send-VkC 0x0D)
Start-Sleep -Seconds 5
"trace-lines=$(@(Get-Content $env:APP_TRACE_FILE -ErrorAction SilentlyContinue).Count)"
"keydbg-lines=$(@(Select-String -Path $env:APP_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue).Count)"
"--- trace tail ---"
Get-Content $env:APP_TRACE_FILE -ErrorAction SilentlyContinue | Select-Object -Last 8
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $targetPid -Force -ErrorAction SilentlyContinue
