import io

p = 'C:/dev/tty/scripts/windows/e2e/run_2b_mouse.ps1'
s = io.open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')

start = s.index("# ---------- k1:")
end = s.index("# ---------- k2:")
k1 = '''# ---------- k1: Ctrl+Insert copies the current selection.
# The drag itself already commits (save_to_clipboard=true), so clear the
# clipboard AFTER the drag: only the key binding can refill it.
$a = Start-Case 'k1-ctrl-insert-copy' 'clip-win.toml'
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2 'echo alpha beta gamma'; Send-Vk2 0x0D
    $null = Wait-Dump2 $a.dump 'alpha beta gamma' 25
    Start-Sleep -Milliseconds 700
    $fr = Find-RowRetry $a.dump 'alpha beta gamma' 8
    $row = $fr.row; $g = $fr.g
    if ($row -ge 0) {
        $geom = Get-Geom $a.hwnd
        $cp = Cell-XY $geom $g.cols $g.rows ($row + 2) 3
        Click-Left $cp.x $cp.y
        Start-Sleep -Milliseconds 400
        $cl = Cell-XY $geom $g.cols $g.rows $row 0
        $cr = Cell-XY $geom $g.cols $g.rows $row 17
        Drag-Left $cl.x $cl.y $cr.x $cr.y
        Start-Sleep -Milliseconds 400
        [System.Windows.Forms.Clipboard]::Clear()
        Start-Sleep -Milliseconds 200
        [WB2]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
        Send-Vk2 0x2D
        [WB2]::keybd_event(0x11, 0, 0x0002, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 600
    }
    $clip = [System.Windows.Forms.Clipboard]::GetText()
    Report2 'k1-ctrl-insert-copy' ($row -ge 0 -and $clip -eq 'alpha beta gamma') "(row=$row clip='$clip')"
    Stop-App2 $a.proc
} else { Report2 'k1-ctrl-insert-copy' $false '(no window)' }
Start-Sleep -Seconds 1

'''
s = s[:start] + k1 + s[end:]

s1start = s.index("# ---------- s1:")
s1end = s.index('Write-Host "==== 2B mouse/sgr summary')
s1 = '''# ---------- s1: SGR mouse reporting. Try each tracking mode; the app only
# reports when is_mouse_tracking() is true for that mode.
function Invoke-Sgr([string]$modeArg, [string]$name) {
    $a = Start-Case $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ ok = $false; bytes = -1; first = '' } }
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
    $ok = Wait-Dump2 $a.dump 'BYTES=' 30
    Start-Sleep -Milliseconds 600
    $txt = Get-Content $a.dump -Raw -Encoding UTF8
    $bytes = -1; $first = ''
    if ($txt -match 'BYTES=(\\d+)') { $bytes = [int]$Matches[1] }
    if ($txt -match 'FIRSTHEX=([0-9A-F\\-]+)') { $first = $Matches[1] }
    Stop-App2 $a.proc
    @{ ok = ($ready -and $ok -and $bytes -gt 0 -and $first.StartsWith('1B-5B-3C')); bytes = $bytes; first = $first }
}
$s1a = Invoke-Sgr 'mouse' 's1a-sgr-1000'
Start-Sleep -Seconds 1
$s1b = Invoke-Sgr 'm1002' 's1b-sgr-1002'
Start-Sleep -Seconds 1
$s1c = Invoke-Sgr 'm1003' 's1c-sgr-1003'
$anyOk = $s1a.ok -or $s1b.ok -or $s1c.ok
Report2 's1-sgr-report' $anyOk "(1000:bytes=$($s1a.bytes) first='$($s1a.first)' | 1002:bytes=$($s1b.bytes) first='$($s1b.first)' | 1003:bytes=$($s1c.bytes) first='$($s1c.first)')"

'''
s = s[:s1start] + s1 + s[s1end:]
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('patched', 'Invoke-Sgr' in s)
