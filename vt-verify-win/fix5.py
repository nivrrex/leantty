import io
p='C:/dev/tty/scripts/windows/e2e/wb2_lib.ps1'
s=io.open(p,encoding='utf-8',newline='').read().replace('\r\n','\n')
old='''            if ($c -and ($c -match $pattern)) { return $true }'''
new='''            # case-sensitive: otherwise AAAA matches leftover aaaa from a paste
            if ($c -and ($c -cmatch $pattern)) { return $true }'''
assert old in s
s=s.replace(old,new,1)
io.open(p,'w',encoding='utf-8',newline='\n').write(s)
print('lib cmatch', '-cmatch' in s)

q='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
t=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')
if 'Clipboard]::Clear()' not in t.split('# ---------- c1:')[1][:200]:
    t=t.replace('''Add-Type -AssemblyName System.Drawing''','''Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Clipboard]::Clear()''',1)
io.open(q,'w',encoding='utf-8',newline='\n').write(t)
print('case clear', 'Clipboard]::Clear()' in t[:900])
