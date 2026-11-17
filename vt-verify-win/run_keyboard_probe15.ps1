# Probe15: alternate keybd_event vs SendInput letters; per-API KEYDBG counts
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\leantty.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WF {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
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

[void][WF]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object WF+INPUT))

$env:LEANTTY_TRACE_FILE = "$OUT\probe15.trace"
$env:LEANTTY_DUMP_SNAPSHOT = "$OUT\probe15.dump"
$env:SHELL = 'cmd.exe'
Remove-Item $env:LEANTTY_TRACE_FILE -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe15.err"
$Script:TargetPid = [uint64]$p.Id
Start-Sleep -Seconds 4

$Script:Wins = @()
$cb = [WF+EnumProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $pidOut = 0
    [void][WF]::GetWindowThreadProcessId($h, [ref]$pidOut)
    if ([uint64]$pidOut -eq $Script:TargetPid) {
        $cls = New-Object System.Text.StringBuilder 256
        [void][WF]::GetClassName($h, $cls, 256)
        if ($cls.ToString() -eq 'Window Class') { $Script:Wins += $h }
    }
    return $true
}
[void][WF]::EnumWindows($cb, [IntPtr]::Zero)
$real = $Script:Wins | Select-Object -First 1
"real=$real"
$tmp = 0
$tid = [WF]::GetWindowThreadProcessId($real, [ref]$tmp)
[void][WF]::ShowWindow($real, 9)
$myTid = [WF]::GetCurrentThreadId()
[void][WF]::AttachThreadInput($myTid, $tid, $true)
[void][WF]::SetFocus($real)
[void][WF]::SetForegroundWindow($real)
[void][WF]::AttachThreadInput($myTid, $tid, $false)
Start-Sleep -Milliseconds 400
"fg=$([WF]::GetForegroundWindow()) match=$([WF]::GetForegroundWindow() -eq $real)"

function Count-Keydbg { @((Select-String -Path $env:LEANTTY_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue)).Count }

# A: keybd_event 'a' (VK 0x41)
[WF]::keybd_event(0x41, 0, 0, [UIntPtr]::Zero)
[WF]::keybd_event(0x41, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 600
"after-keybd-a: $(Count-Keydbg)"

# B: SendInput 'b' (VK 0x42)
$n = 2
$inputs = New-Object WF+INPUT[] $n
$inputs[0] = New-Object WF+INPUT
$inputs[0].type = 1; $inputs[0].u.ki.wVk = [uint16]0x42; $inputs[0].u.ki.dwFlags = 0
$inputs[1] = New-Object WF+INPUT
$inputs[1].type = 1; $inputs[1].u.ki.wVk = [uint16]0x42; $inputs[1].u.ki.dwFlags = 0x0002
$s = [uint32][WF]::SendInput($n, $inputs, $Script:INPUT_SIZE)
Start-Sleep -Milliseconds 600
"after-sendinput-b(sent=$s): $(Count-Keydbg)"

# C: keybd_event 'c' (VK 0x43)
[WF]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
[WF]::keybd_event(0x43, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 600
"after-keybd-c: $(Count-Keydbg)"

# D: SendInput scancode 'd' (scan 0x20)
$inputs[0].u.ki.wVk = 0; $inputs[0].u.ki.wScan = 0x20; $inputs[0].u.ki.dwFlags = 0x0008
$inputs[1].u.ki.wVk = 0; $inputs[1].u.ki.wScan = 0x20; $inputs[1].u.ki.dwFlags = 0x0008 -bor 0x0002
$s2 = [uint32][WF]::SendInput($n, $inputs, $Script:INPUT_SIZE)
Start-Sleep -Milliseconds 600
"after-sendinput-scancode-d(sent=$s2): $(Count-Keydbg)"

"--- KEYDBG lines ---"
Select-String -Path $env:LEANTTY_TRACE_FILE -Pattern 'KEYDBG' -ErrorAction SilentlyContinue | Select-Object -First 8 | ForEach-Object { $_.Line }
"--- dump ---"
Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 5
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
