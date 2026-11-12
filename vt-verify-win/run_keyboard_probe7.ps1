# Probe6: is the keyboard FOCUS really on the app window? GetGUIThreadInfo + PostMessage control + keybd_event control
$ErrorActionPreference = 'Continue'
$ROOT = 'C:\dev\tty'
$OUT = 'C:\dev\tty\vt-verify-win\logs\e2e-probe'
New-Item -ItemType Directory -Force -Path $OUT | Out-Null
$AppBin = "$ROOT\target\debug\app.exe"

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class W8 {
    [DllImport("user32.dll")] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO lpgui);
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

[void][W8]::SystemParametersInfo(0x2001, 0, [IntPtr]::Zero, 0)
$Script:INPUT_SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf((New-Object W8+INPUT))

function Focus-App([IntPtr]$h) {
    [void][W8]::ShowWindow($h, 9)
    [W8]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [W8]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    $tmp = 0
    $appTid = [W8]::GetWindowThreadProcessId($h, [ref]$tmp)
    $myTid = [W8]::GetCurrentThreadId()
    if ($appTid -ne $myTid) { [void][W8]::AttachThreadInput($myTid, $appTid, $true) }
    [void][W8]::SetFocus($h)
    $r = [W8]::SetForegroundWindow($h)
    if ($appTid -ne $myTid) { [void][W8]::AttachThreadInput($myTid, $appTid, $false) }
    return $r
}

function GuiInfo([IntPtr]$h) {
    $tmp = 0
    $tid = [W8]::GetWindowThreadProcessId($h, [ref]$tmp)
    $gui = New-Object W8+GUITHREADINFO
    $gui.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($gui)
    if ([W8]::GetGUIThreadInfo($tid, [ref]$gui)) {
        "tid=$tid active=$($gui.hwndActive) focus=$($gui.hwndFocus) match=$($gui.hwndFocus -eq $h)"
    } else {
        "tid=$tid guiinfo-failed"
    }
}

function Send-Vk([uint32]$vk) {
    $n = 2
    $inputs = New-Object W8+INPUT[] $n
    $inputs[0] = New-Object W8+INPUT
    $inputs[0].type = 1
    $inputs[0].u.ki.wVk = [uint16]$vk
    $inputs[0].u.ki.dwFlags = 0
    $inputs[1] = New-Object W8+INPUT
    $inputs[1].type = 1
    $inputs[1].u.ki.wVk = [uint16]$vk
    $inputs[1].u.ki.dwFlags = 0x0002
    return [uint32][W8]::SendInput($n, $inputs, $Script:INPUT_SIZE)
}

$env:APP_DUMP_SNAPSHOT = "$OUT\probe6.dump"
$env:SHELL = 'cmd.exe'
$p = Start-Process -FilePath $AppBin -WorkingDirectory $ROOT -PassThru -RedirectStandardError "$OUT\probe6.err"
$hwnd = [IntPtr]::Zero
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 30) {
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($proc -and $proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle; break }
    Start-Sleep -Milliseconds 500
}
"hwnd=$hwnd"
$r = Focus-App $hwnd
"focus-result=$r"
"gui-before: $(GuiInfo $hwnd)"

# control test 1: PostMessage WM_CHAR 'p' directly to the window
[void][W8]::PostMessage($hwnd, 0x0102, [IntPtr]([uint16][char]'p'), [IntPtr]1)
Start-Sleep -Milliseconds 500
"after-postmessage-p: $(GuiInfo $hwnd)"

# test 2: SendInput VK 'x'
$s = Send-Vk 0x58
"sendinput-x=$s"
Start-Sleep -Milliseconds 500

# test 3: legacy keybd_event 'z'
[W8]::keybd_event(0x5A, 0, 0, [UIntPtr]::Zero)
[W8]::keybd_event(0x5A, 0, 2, [UIntPtr]::Zero)
"keybd-event-z done"
Start-Sleep -Milliseconds 500

# test 4: SendInput with KEYEVENTF_SCANCODE for 'c'
$n = 2
$inputs = New-Object W8+INPUT[] $n
$inputs[0] = New-Object W8+INPUT
$inputs[0].type = 1
$inputs[0].u.ki.wVk = 0
$inputs[0].u.ki.wScan = 0x2E
$inputs[0].u.ki.dwFlags = 0x0008
$inputs[1] = New-Object W8+INPUT
$inputs[1].type = 1
$inputs[1].u.ki.wVk = 0
$inputs[1].u.ki.wScan = 0x2E
$inputs[1].u.ki.dwFlags = 0x0008 -bor 0x0002
$s3 = [uint32][W8]::SendInput($n, $inputs, $Script:INPUT_SIZE)
"sendinput-scancode-c=$s3"
Start-Sleep -Seconds 5
"gui-after: $(GuiInfo $hwnd)"
"--- dump ---"
Get-Content $env:APP_DUMP_SNAPSHOT -ErrorAction SilentlyContinue | Select-Object -First 4
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
