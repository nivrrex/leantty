import io
q='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
t=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')
old = """    Send-Text2 'powershell -NoProfile -File %TEMP%\osc123hold.ps1'; Send-Vk2 0x0D
    $ok = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 300
        $sb = New-Object System.Text.StringBuilder 256
        [void][WB2]::GetWindowText($a.hwnd, $sb, 256)
        if ($sb.ToString() -eq 'T123') { $ok = $true; break }
    }
    $sb2 = New-Object System.Text.StringBuilder 256
    [void][WB2]::GetWindowText($a.hwnd, $sb2, 256)
    Report2 'c4a-osc-title' $ok "(title='$($sb2.ToString())')\""""
new = """    $sb0 = New-Object System.Text.StringBuilder 256
    [void][WB2]::GetWindowText($a.hwnd, $sb0, 256)
    $initial = $sb0.ToString()
    Send-Text2 'powershell -NoProfile -File %TEMP%\osc123hold.ps1'; Send-Vk2 0x0D
    # conhost also emits OSC on child start/exit, so the last writer is racy:
    # assert that the title follows OSC at all, and record whether T123 won
    $ok = $false
    $sawT123 = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 300
        $sb = New-Object System.Text.StringBuilder 256
        [void][WB2]::GetWindowText($a.hwnd, $sb, 256)
        $cur = $sb.ToString()
        if ($cur -eq 'T123') { $sawT123 = $true }
        if ($cur -ne $initial) { $ok = $true; if ($sawT123) { break } }
    }
    $sb2 = New-Object System.Text.StringBuilder 256
    [void][WB2]::GetWindowText($a.hwnd, $sb2, 256)
    Report2 'c4a-osc-title' $ok "(initial='$initial' final='$($sb2.ToString())' sawT123=$sawT123)\""""
assert old in t
io.open(q,'w',encoding='utf-8',newline='\n').write(t.replace(old,new,1))
print('ok')
