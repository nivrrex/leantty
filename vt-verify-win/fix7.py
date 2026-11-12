import io
q='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
t=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')
old = """$a = Start-Case 'c5-paste-10mb-typing' 'clip-win.toml' $null
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    $pout = Join-Path $OUT 'c5-probe.txt'
    Remove-Item $pout -ErrorAction SilentlyContinue
    $env:PROBE_OUT = $pout
    Send-Text2"""
new = """# the probe inherits the environment from the app, so PROBE_OUT has to be set
# before the app process is created
$pout = Join-Path $OUT 'c5-probe.txt'
Remove-Item $pout -ErrorAction SilentlyContinue
$env:PROBE_OUT = $pout
$a = Start-Case 'c5-paste-10mb-typing' 'clip-win.toml' $null
$Script:CurHwnd = $a.hwnd
if ($a.hwnd -ne [IntPtr]::Zero) {
    Send-Text2"""
assert old in t
t = t.replace(old, new, 1)
io.open(q,'w',encoding='utf-8',newline='\n').write(t)
print('ok')
