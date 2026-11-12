# Probe19: faithful real-console Ctrl+C control via CONIN$ WriteConsoleInput
# (AttachConsole to child console + type the Ctrl+C key event into its input buffer)
$ErrorActionPreference = 'Continue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class K2 {
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint dwProcessId);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr CreateFile(string lpFileName, uint dwDesiredAccess, uint dwShareMode, IntPtr lpSecurityAttributes, uint dwCreationDisposition, uint dwFlagsAndAttributes, IntPtr hTemplateFile);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool WriteConsoleInput(IntPtr hConsoleInput, INPUT_RECORD[] lpBuffer, uint nLength, out uint lpNumberOfEventsWritten);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetLastError();
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct KEY_EVENT_RECORD {
        public bool bKeyDown; public ushort wRepeatCount; public ushort wVirtualKeyCode;
        public ushort wVirtualScanCode; public char UnicodeChar; public uint dwControlKeyState;
    }
    [StructLayout(LayoutKind.Explicit)] public struct INPUT_RECORD {
        [FieldOffset(0)] public ushort EventType;
        [FieldOffset(4)] public KEY_EVENT_RECORD KeyEvent;
    }
}
'@
$GENERIC_READ = 0x80000000; $GENERIC_WRITE = 0x40000000
$OPEN_EXISTING = 3; $FILE_SHARE_READ = 1; $FILE_SHARE_WRITE = 2
$LEFT_CTRL_PRESSED = 0x0008

function Send-RealCtrlC([uint32]$childPid) {
    [void][K2]::AttachConsole($childPid)
    $h = [K2]::CreateFile('CONIN$', $GENERIC_READ -bor $GENERIC_WRITE, $FILE_SHARE_READ -bor $FILE_SHARE_WRITE, [IntPtr]::Zero, $OPEN_EXISTING, 0, [IntPtr]::Zero)
    if ($h -eq [IntPtr](-1) -or $h -eq [IntPtr]::Zero) { "conin-open-failed"; [void][K2]::FreeConsole(); return }
    $n = 2
    $recs = New-Object K2+INPUT_RECORD[] $n
    # key down: Ctrl+C (char 0x03, VK 0x43, left ctrl held)
    $recs[0] = New-Object K2+INPUT_RECORD
    $recs[0].EventType = 0x0001
    $recs[0].KeyEvent.bKeyDown = $true
    $recs[0].KeyEvent.wRepeatCount = 1
    $recs[0].KeyEvent.wVirtualKeyCode = 0x43
    $recs[0].KeyEvent.wVirtualScanCode = 0x2E
    $recs[0].KeyEvent.UnicodeChar = [char]0x0003
    $recs[0].KeyEvent.dwControlKeyState = $LEFT_CTRL_PRESSED
    # key up
    $recs[1] = New-Object K2+INPUT_RECORD
    $recs[1].EventType = 0x0001
    $recs[1].KeyEvent.bKeyDown = $false
    $recs[1].KeyEvent.wRepeatCount = 1
    $recs[1].KeyEvent.wVirtualKeyCode = 0x43
    $recs[1].KeyEvent.wVirtualScanCode = 0x2E
    $recs[1].KeyEvent.UnicodeChar = [char]0x0003
    $recs[1].KeyEvent.dwControlKeyState = $LEFT_CTRL_PRESSED
    $written = 0
    $ok = [K2]::WriteConsoleInput($h, $recs, $n, [ref]$written)
    "conin-write ok=$ok written=$written err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    [void][K2]::FreeConsole()
}

foreach ($case in @(@{ n = 'sleep'; exe = 'powershell'; args = @('-NoProfile', '-Command', 'Start-Sleep -Seconds 60') },
                    @{ n = 'ping';  exe = 'powershell'; args = @('-NoProfile', '-Command', 'ping.exe -t 127.0.0.1') })) {
    $p = Start-Process $case.exe -ArgumentList $case.args -PassThru
    Start-Sleep -Seconds 4
    "$($case.n)-alive-before=$(-not $p.HasExited)"
    Send-RealCtrlC ([uint32]$p.Id)
    Start-Sleep -Seconds 5
    "$($case.n)-alive-after=$(-not $p.HasExited) exitcode=$(if ($p.HasExited) { "$($p.ExitCode)" } else { 'n/a' })"
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
}
