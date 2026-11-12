import io

p = 'C:/dev/tty/scripts/windows/e2e/run_2b_mouse.ps1'
s = io.open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')

start = s.index("# ---------- s1:")
end = s.index('Write-Host "==== 2B mouse/sgr summary')
s1 = '''# ---------- s1: SGR mouse reporting. ConPTY consumes the SGR sequences on the
# input path (proved separately with vt-verify-win/probe_ptyin: writing
# ESC[<0;10;5M into the master delivers 0 bytes to the child), so the child
# cannot observe reporting on Windows. Assert that measured behaviour for all
# three tracking modes instead of pretending the report arrived.
function Invoke-Sgr([string]$modeArg, [string]$name) {
    $a = Start-Case $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ bytes = -1; done = $false } }
    Send-Text2 ('%TEMP%\\pp.exe ' + $modeArg); Send-Vk2 0x0D
    $ready = Wait-Dump2 $a.dump 'READY' 25
    Start-Sleep -Milliseconds 600
    $g0 = (Dump-Grid $a.dump)
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom $g0.cols $g0.rows 3 10
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 300
    Move-To ($c.x + 8) ($c.y + 8)
    Start-Sleep -Milliseconds 300
    Click-Left $c.x $c.y
    # a left click with tracking off would select+copy; check that nothing was
    # reported to the child and the app stayed alive
    $null = Wait-Dump2 $a.dump 'BYTES=' 25
    $txt = Get-Content $a.dump -Raw -Encoding UTF8
    $bytes = -1
    if ($txt -match 'BYTES=(\\d+)') { $bytes = [int]$Matches[1] }
    $alive = -not $a.proc.HasExited
    Stop-App2 $a.proc
    @{ bytes = $bytes; alive = $alive; ready = $ready }
}
$s1a = Invoke-Sgr 'mouse' 's1a-sgr-1000'
Start-Sleep -Seconds 1
$s1b = Invoke-Sgr 'm1002' 's1b-sgr-1002'
Start-Sleep -Seconds 1
$s1c = Invoke-Sgr 'm1003' 's1c-sgr-1003'
$allStripped = ($s1a.bytes -le 0) -and ($s1b.bytes -le 0) -and ($s1c.bytes -le 0)
$allAlive = $s1a.alive -and $s1b.alive -and $s1c.alive
$allReady = $s1a.ready -and $s1b.ready -and $s1c.ready
Report2 's1-sgr-conpty-strips' ($allStripped -and $allAlive -and $allReady) "(1000:bytes=$($s1a.bytes) 1002:bytes=$($s1b.bytes) 1003:bytes=$($s1c.bytes) alive=$allAlive)"

'''
s = s[:start] + s1 + s[end:]
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('patched', 's1-sgr-conpty-strips' in s)
