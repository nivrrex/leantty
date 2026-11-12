# Probe18: control test - does Start-Sleep respond to a REAL console Ctrl+C?
# (AttachConsole to child + GenerateConsoleCtrlEvent; no ConPTY, no app)
$ErrorActionPreference = 'Continue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class KC {
    [DllImport("kernel32.dll")] public static extern bool AttachConsole(uint dwProcessId);
    [DllImport("kernel32.dll")] public static extern bool FreeConsole();
    [DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);
    [DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
}
'@
$CTRL_C = 0

# control 1: Start-Sleep
$p1 = Start-Process powershell -ArgumentList @('-NoProfile', '-Command', 'Start-Sleep -Seconds 60; "SLEEP-DONE"') -PassThru
Start-Sleep -Seconds 4
"sleep-alive-before=$(-not $p1.HasExited)"
[void][KC]::AttachConsole([uint32]$p1.Id)
$gen = [KC]::GenerateConsoleCtrlEvent($CTRL_C, 0)
[void][KC]::FreeConsole()
"sleep-ctrl-event-sent=$gen"
Start-Sleep -Seconds 5
"sleep-alive-after=$(-not $p1.HasExited) exitcode=$(if ($p1.HasExited) { $p1.ExitCode } else { 'n/a' })"
if (-not $p1.HasExited) { Stop-Process -Id $p1.Id -Force -ErrorAction SilentlyContinue }

# control 2: ping -t (should die under real console Ctrl+C)
$p2 = Start-Process cmd -ArgumentList @('/c', 'ping -t 127.0.0.1') -PassThru
Start-Sleep -Seconds 4
"ping-alive-before=$(-not $p2.HasExited)"
[void][KC]::AttachConsole([uint32]$p2.Id)
$gen2 = [KC]::GenerateConsoleCtrlEvent($CTRL_C, 0)
[void][KC]::FreeConsole()
"ping-ctrl-event-sent=$gen2"
Start-Sleep -Seconds 5
"ping-alive-after=$(-not $p2.HasExited) exitcode=$(if ($p2.HasExited) { $p2.ExitCode } else { 'n/a' })"
if (-not $p2.HasExited) { Stop-Process -Id $p2.Id -Force -ErrorAction SilentlyContinue }
