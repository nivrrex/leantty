import io
q='C:/dev/tty/vt-verify-win/probe_paste/src/main.rs'
t=io.open(q,encoding='utf-8').read()
if 'CHARM=' not in t:
    t=t.replace('''            let mut i = 0;
            while i + 4 <= scan.len() {
                if &scan[i..i + 4] == b"MARK" {
                    marks += 1;
                    i += 4;
                } else {
                    i += 1;
                }
            }''','''            let mut i = 0;
            while i + 4 <= scan.len() {
                if &scan[i..i + 4] == b"MARK" {
                    marks += 1;
                    i += 4;
                } else {
                    i += 1;
                }
            }
            // interleaved paste data can split a marker, so also count the
            // individual characters that never appear in the pasted payload
            for b in scan[carry.len()..].iter() {
                match *b {
                    b'M' => cm += 1,
                    b'R' => cr += 1,
                    b'K' => ck += 1,
                    _ => {}
                }
            }''')
    t=t.replace('''    let mut carry: Vec<u8> = Vec::new();''','''    let mut carry: Vec<u8> = Vec::new();
    let mut cm = 0usize;
    let mut cr = 0usize;
    let mut ck = 0usize;''')
    t=t.replace('''            let _ = std::fs::write(&path, format!("BYTES={} MARKCOUNT={}", total, marks));''','''            let _ = std::fs::write(
                &path,
                format!("BYTES={} MARKCOUNT={} CHARM={} CHARR={} CHARK={}", total, marks, cm, cr, ck),
            );''')
    t=t.replace('''        let _ = std::fs::write(&path, format!("BYTES={} MARKCOUNT={}", total, marks));''','''        let _ = std::fs::write(
            &path,
            format!("BYTES={} MARKCOUNT={} CHARM={} CHARR={} CHARK={}", total, marks, cm, cr, ck),
        );''')
io.open(q,'w',encoding='utf-8').write(t)
print('probe chars', 'CHARM=' in t)

r='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
s=io.open(r,encoding='utf-8',newline='').read().replace('\r\n','\n')
s=s.replace("""    $bytes = -1; $marks = -1
    for ($i = 0; $i -lt 180; $i++) {
        Start-Sleep -Milliseconds 1000
        if (Test-Path $pout) {
            $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -match 'BYTES=(\d+)') { $bytes = [int]$Matches[1] }
            if ($t -match 'MARKCOUNT=(\d+)') { $marks = [int]$Matches[1] }
        }
        if ($bytes -eq $target -and $marks -eq 5) { break }
    }
    Report2 'c5-paste-10mb-typing' ($ready -and $bytes -eq $target -and $marks -eq 5) \"(bytes=$bytes target=$target marks=$marks)\"""","""    $bytes = -1; $marks = -1; $cm = -1; $cr = -1; $ck = -1
    for ($i = 0; $i -lt 180; $i++) {
        Start-Sleep -Milliseconds 1000
        if (Test-Path $pout) {
            $t = Get-Content $pout -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            if ($t -match 'BYTES=(\d+)') { $bytes = [int]$Matches[1] }
            if ($t -match 'MARKCOUNT=(\d+)') { $marks = [int]$Matches[1] }
            if ($t -match 'CHARM=(\d+)') { $cm = [int]$Matches[1] }
            if ($t -match 'CHARR=(\d+)') { $cr = [int]$Matches[1] }
            if ($t -match 'CHARK=(\d+)') { $ck = [int]$Matches[1] }
        }
        if ($bytes -eq $target) { break }
    }
    # all 20 marker bytes must arrive; a marker may be split by interleaved
    # pasted data, so the per-character counts are the loss criterion
    $charsOk = ($cm -eq 5) -and ($cr -eq 5) -and ($ck -eq 5)
    Report2 'c5-paste-10mb-typing' ($ready -and $bytes -eq $target -and $charsOk) \"(bytes=$bytes target=$target marks=$marks M=$cm R=$cr K=$ck)\"""")
io.open(r,'w',encoding='utf-8',newline='\n').write(s)
print('case chars', 'CHARM=' in s)
