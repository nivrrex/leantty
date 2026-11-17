# Probe13: fixed EnumWindows pid filter -> real window -> focus it -> SendInput letters -> trace KEYDBG
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\leantty.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WD {
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
}
'@

[void][WD]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WD+INPUT))

$env:LEANTTY_TRACE_FILE = "$OUT\probe13.trace"
$env:LEANTTY_DUMP_SNAPSHOT = "$OUT\probe13.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:LEANTTY_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe13.err"
$Script:TargetPid = [uint64]$p.Id
"pid=$Script:TargetPid"
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WD+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WD]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        $tt = New-Object System.Text.StringBuilder 256
        [void][WD]::GetClassName($h, $cls, 256)
        [void][WD]::GetWindowText($h, $tt, 256)
        $r = New-Object WD+RECT
        [void][WD]::GetWindowRect($h, [ref]$r)
        $pidOut2 = 0
        $tid = [WD]::GetWindowThreadProcessId($h, [ref]$pidOut2)
        $Script:Wins += [PSCustomObject]@{ hwnd = $h; tid = $tid; cls = $cls.ToString(); title = $tt.ToString(); vis = [WD]::IsWindowVisible($h); w = $r.right - $r.left; h = $r.bottom - $r.top }
    }
    return $true
}
[void][WD]::EnumWindows($cb, [IntPtr]::Zero)
"--- windows of pid ---"
$Script:Wins | ForEach-Object { "hwnd=$($_.hwnd) tid=$($_.tid) w=$($_.w) h=$($_.h) vis=$($_.vis) cls=$($_.cls) title='$($_.title)'" }

$real = $Script:Wins | Where-Object { $_.w -gt 100 -and $_.h -gt 100 } | Select-Object -First 1
if ($null -eq $real) { "NO-REAL-WINDOW"; Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; exit 1 }
"real-hwnd=$($real.hwnd) title='$($real.title)'"

# focus REAL window (proven recipe), verify via GetForegroundWindow
[void][WD]::ShowWindow($real.hwnd, 9)
$myTid = [WD]::GetCurrentThreadId()
if ($real.tid -ne $myTid) { [void][WD]::AttachThreadInput($myTid, $real.tid, $true) }
[void][WD]::SetFocus($real.hwnd)
$fr = [WD]::SetForegroundWindow($real.hwnd)
if ($real.tid -ne $myTid) { [void][WD]::AttachThreadInput($myTid, $real.tid, $false) }
Start-Sleep -Milliseconds 400
$fgNow = [WD]::GetForegroundWindow()
"setFg=$fr fgNow=$fgNow match=$($fgNow -eq $real.hwnd)"

# pure SendInput VK letters (NO keybd_event)
function Send-VkD([uint32]$vk) {
    $n = 2
    $inputs = New-Object WD+INPUT[] $n
    $inputs[0] = New-Object WD+INPUT
    $inputs[0].type = 1; $inputs[0].u.ki.wVk = [uint16]$vk; $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object WD+INPUT
    $inputs[1].type = 1; $inputs[1].u.ki.wVk = [uint16]$vk; $inputs[1].u.ki.dwFlags = 0x0002
    [uint32][WD]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}
foreach ($ch in 'echo hello'.ToCharArray()) {
    $vk = [uint32][byte][char]::ToUpper($ch)
    if ($ch -eq ' ') { $vk = 0x20 }
    $s = Send-VkD $vk
    "vk=0x{0:X} sent=$s" -f $vk
    Start-Sleep -Milliseconds 60
}
[uint32](Send-VkD 0x0D) | Out-Null
"Enter sent"
Start-Sleep -Seconds 5
"trace-lines=$(@(Get-Content $env:LEANTTY_TRACE_FILE -ErrorAction SilentlyContinue).Count)"
"keydbg-lines=$(@(Select-String -Path $env:LEANTTY_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue).Count)"
"--- KEYDBG lines ---"
Select-String -Path $env:LEANTTY_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue | Select-Object -First 6 | ForEach-Object { $_.Line }
"--- dump ---"
Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 5
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
