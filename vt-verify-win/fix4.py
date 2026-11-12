import io
q='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
t=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')

# c1: wait for the prompt before the second command, and search rows more leniently
t=t.replace("""    $okA = Wait-Dump2 $a.dump 'AAAAAAAAAA' 30
    Start-Sleep -Milliseconds 600
    Send-Text2 'powershell -NoProfile -Command \"''B1'';''B2'';''B3''\"'; Send-Vk2 0x0D
    $okB = Wait-Dump2 $a.dump 'B3' 30""","""    $okA = Wait-Dump2 $a.dump 'AAAAAAAAAA' 30
    # wait for the prompt to come back before typing the next command
    Start-Sleep -Milliseconds 1200
    Send-Text2 'powershell -NoProfile -Command \"''B1'';''B2'';''B3''\"'; Send-Vk2 0x0D
    $okB = Wait-Dump2 $a.dump 'B3' 30
    if (-not $okB) { Start-Sleep -Milliseconds 1500 }""")

# c5: read the probe's own report file instead of scraping the screen
t=t.replace("""    Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
    $ready = Wait-Dump2 $a.dump 'READY' 25
    $big = 'a' * (10 * 1024 * 1024)""","""    $pout = Join-Path $OUT 'c5-probe.txt'
    Remove-Item $pout -ErrorAction SilentlyContinue
    $env:PROBE_OUT = $pout
    Send-Text2 '%TEMP%\pp.exe'; Send-Vk2 0x0D
    $ready = Wait-Dump2 $a.dump 'READY' 25
    $big = 'a' * (10 * 1024 * 1024)""")
t=t.replace("""    $target = 10 * 1024 * 1024 + 20
    $bytes = -1; $marks = -1
    for ($i = 0; $i -lt 120; $i++) {
        Start-Sleep -Milliseconds 1000
        $t = Get-Content $a.dump -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if (-not $t) { continue }
        foreach ($x in [regex]::Matches($t, 'BYTES=(\d+)')) {
            $v = [int]$x.Groups[1].Value
            if ($v -gt $bytes) { $bytes = $v }
        }
        foreach ($x in [regex]::Matches($t, 'MARKCOUNT=(\d+)')) {
            $v = [int]$x.Groups[1].Value
            if ($v -gt $marks) { $marks = $v }
        }
        if ($bytes -ge $target -and $marks -ge 5) { break }
    }
    Report2 'c5-paste-10mb-typing' ($ready -and $bytes -eq $target -and $marks -eq 5) \"(bytes=$bytes target=$target marks=$marks)\"""","""    $target = 10 * 1024 * 1024 + 20
    $bytes = -1; $marks = -1
    for ($i = 0; $i -lt 180; $i++) {
        Start-Sleep -Milliseconds 1000
        if (Test-Path $pout) {
            $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -match 'BYTES=(\d+)') { $bytes = [int]$Matches[1] }
            if ($t -match 'MARKCOUNT=(\d+)') { $marks = [int]$Matches[1] }
        }
        if ($bytes -eq $target -and $marks -eq 5) { break }
    }
    Report2 'c5-paste-10mb-typing' ($ready -and $bytes -eq $target -and $marks -eq 5) \"(bytes=$bytes target=$target marks=$marks)\"""")
io.open(q,'w',encoding='utf-8',newline='\n').write(t)
print('c1 prompt wait', 'wait for the prompt to come back' in t)
print('c5 probe file', 'c5-probe.txt' in t)
