# Instrumented single-case probe: where do SendInput keys go?
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W4 {
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
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

$env:APP_DUMP_SNAPSHOT = "$OUT\probe.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe.err"
"pid=$($p.Id)"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"

# foreground sequence with full reporting
[void][W4]::ShowWindow($hwnd, 9)
[W4]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
[W4]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
$tmp = 0
$fg = [W4]::GetForegroundWindow()
"fg_before=$fg"
if ($fg -ne [IntPtr]::Zero) {
    $fgTid = [W4]::GetWindowThreadProcessId($fg, [ref]$tmp)
    $myTid = [W4]::GetCurrentThreadId()
    "fgTid=$fgTid myTid=$myTid"
    if ($fgTid -ne $myTid) {
        "attach=$([W4]::AttachThreadInput($myTid, $fgTid, $true))"
        "setFg=$([W4]::SetForegroundWindow($hwnd))"
        [void][W4]::AttachThreadInput($myTid, $fgTid, $false)
    } else {
        "setFg=$([W4]::SetForegroundWindow($hwnd))"
    }
} else {
    "setFg=$([W4]::SetForegroundWindow($hwnd))"
}
"fg_after=$([W4]::GetForegroundWindow())"

# send a single char 'x' then Enter, watch dump + err
$n = 2
$inputs = New-Object W4+INPUT[] $n
$inputs[0] = New-Object W4+INPUT
$inputs[0].type = 1
$inputs[0].u.ki.wVk = 0
$inputs[0].u.ki.wScan = [ushort][char]'x'
$inputs[0].u.ki.dwFlags = 0x0004
$inputs[1] = New-Object W4+INPUT
$inputs[1].type = 1
$inputs[1].u.ki.wVk = 0
$inputs[1].u.ki.wScan = [ushort][char]'x'
$inputs[1].u.ki.dwFlags = 0x0004 -bor 0x0002
$sent = [W4]::SendInput($n, $inputs, [System.Runtime.InteropServices.Marshal]::SizeOf([W4+INPUT]))
"sendInput_sent=$sent of $n"
$inputs[0].u.ki.wScan = [ushort][char]"`r"
$inputs[0].u.ki.dwFlags = 0x0004
$inputs[1].u.ki.wScan = [ushort][char]"`r"
$inputs[1].u.ki.dwFlags = 0x0004 -bor 0x0002
[void][W4]::SendInput(2, $inputs, [System.Runtime.InteropServices.Marshal]::SizeOf([W4+INPUT]))
Start-Sleep -Seconds 8
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 6
"--- err ---"
Get-Content "$OUT\probe.err" -ErrorAction SilentlyContinue | Select-Object -First 6
"fg_final=$([W4]::GetForegroundWindow()) hwnd=$hwnd"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
