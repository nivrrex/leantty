# Probe2: fixed SizeOf(instance) + plain SetForegroundWindow + keys + full reporting
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\leantty.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W4 {
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
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

$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object W4+INPUT))
"input_size=$Script:INPUT_SIZE"

$env:LEANTTY_DUMP_SNAPSHOT = "$OUT\probe2.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe2.err"
"pid=$($p.Id)"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
[void][W4]::ShowWindow($hwnd, 9)
"setFg=$([W4]::SetForegroundWindow($hwnd))"
Start-Sleep -Milliseconds 400
"fg_after=$([W4]::GetForegroundWindow())"
"match=$($hwnd -eq [W4]::GetForegroundWindow())"

$n = 2
$inputs = New-Object W4+INPUT[] $n
$inputs[0] = New-Object W4+INPUT
$inputs[0].type = 1
$inputs[0].u.ki.wVk = 0
$inputs[0].u.ki.wScan = [char]'x'
$inputs[0].u.ki.dwFlags = 0x0004
$inputs[1] = New-Object W4+INPUT
$inputs[1].type = 1
$inputs[1].u.ki.wVk = 0
$inputs[1].u.ki.wScan = [char]'x'
$inputs[1].u.ki.dwFlags = 0x0004 -bor 0x0002
$sent = [uint32][W4]::SendInput($n, $inputs, $Script:INPUT_SIZE)
"sendInput_sent=$sent of $n"
$inputs[0].u.ki.wScan = [char]"`r"
$inputs[0].u.ki.dwFlags = 0x0004
$inputs[1].u.ki.wScan = [char]"`r"
$inputs[1].u.ki.dwFlags = 0x0004 -bor 0x0002
$sent2 = [uint32][W4]::SendInput(2, $inputs, $Script:INPUT_SIZE)
"sendInput_sent2=$sent2 of 2"
Start-Sleep -Seconds 8
"--- dump ---"
Get-Content $env:LEANTTY_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 6
"--- err ---"
Get-Content "$OUT\probe2.err" -ErrorAction SilentlyContinue | Select-Object -First 6
"fg_final=$([W4]::GetForegroundWindow()) hwnd=$hwnd"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
