import io

p = 'C:/dev/tty/scripts/windows/e2e/run_2b_clip.ps1'
s = io.open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')

start = s.index("# ---------- n4:")
end = s.index("# ---------- n5:")
new = '''# ---------- n4: 1 MB paste (2D.2 drain fix). The reader reports its running
# total repeatedly, so take the maximum seen rather than the last match.
function Get-MaxBytes([string]$dump) {
    $t = Get-Content $dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $t) { return -1 }
    $mx = -1
    foreach ($x in [regex]::Matches($t, 'BYTES=(\\d+)')) {
        $v = [int]$x.Groups[1].Value
        if ($v -gt $mx) { $mx = $v }
    }
    $mx
}

function Invoke-1MbPaste([bool]$wiggle) {
    $name = if ($wiggle) { 'n4b-paste-1mb-busy' } else { 'n4a-paste-1mb-idle' }
    $a = Start-Case $name 'clip-win.toml'
    $Script:CurHwnd = $a.hwnd
    if ($a.hwnd -eq [IntPtr]::Zero) { return @{ ready = $false; bytes = -1; ok = $false } }
    Send-Text2 '%TEMP%\\pp.exe'; Send-Vk2 0x0D
    $ready = Wait-Dump2 $a.dump 'READY' 25
    [System.Windows.Forms.Clipboard]::SetText('a' * 1048576)
    Start-Sleep -Milliseconds 300
    $g0 = (Dump-Grid $a.dump)
    $geom = Get-Geom $a.hwnd
    $c = Cell-XY $geom $g0.cols $g0.rows 1 40
    Click-Right $c.x $c.y
    $bytes = -1
    for ($i = 0; $i -lt 90; $i++) {
        Start-Sleep -Milliseconds 1000
        if ($wiggle) { Move-To ($c.x + ($i % 7)) ($c.y + ($i % 5)) }
        $bytes = Get-MaxBytes $a.dump
        if ($bytes -ge 1048576) { break }
    }
    $alive = -not $a.proc.HasExited
    Stop-App2 $a.proc
    @{ ready = $ready; bytes = $bytes; ok = ($ready -and $bytes -eq 1048576 -and $alive) }
}

$r1 = Invoke-1MbPaste $false
Report2 'n4a-paste-1mb-idle' $r1.ok "(bytes=$($r1.bytes) of 1048576, no extra input events)"
Start-Sleep -Seconds 1
$r2 = Invoke-1MbPaste $true
Report2 'n4b-paste-1mb-busy' $r2.ok "(bytes=$($r2.bytes) of 1048576, with mouse motion)"
Start-Sleep -Seconds 1

'''
s = s[:start] + new + s[end:]

# strengthen m2: up to 3 attempts, always re-priming
old_m2 = '''        for ($attempt = 1; $attempt -le 2; $attempt++) {'''
new_m2 = '''        for ($attempt = 1; $attempt -le 3; $attempt++) {'''
if old_m2 in s:
    s = s.replace(old_m2, new_m2, 1)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok', 'Get-MaxBytes' in s)
