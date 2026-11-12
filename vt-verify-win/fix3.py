import io

# 1) lib: also copy the hold variants of the OSC scripts
p='C:/dev/tty/scripts/windows/e2e/wb2_lib.ps1'
s=io.open(p,encoding='utf-8',newline='').read().replace('\r\n','\n')
needle="Copy-Item (Join-Path $ROOT 'vt-verify-win\osc999.ps1') (Join-Path $env:TEMP 'osc999.ps1') -Force"
add=needle+"\nCopy-Item (Join-Path $ROOT 'vt-verify-win\osc123hold.ps1') (Join-Path $env:TEMP 'osc123hold.ps1') -Force\nCopy-Item (Join-Path $ROOT 'vt-verify-win\osc999hold.ps1') (Join-Path $env:TEMP 'osc999hold.ps1') -Force"
if 'osc123hold' not in s and needle in s:
    s=s.replace(needle, add, 1)
io.open(p,'w',encoding='utf-8',newline='\n').write(s)
print('lib hold', 'osc123hold' in s)

# 2) probe: write the running report to PROBE_OUT so the e2e does not scrape the screen
q='C:/dev/tty/vt-verify-win/probe_paste/src/main.rs'
t=io.open(q,encoding='utf-8').read()
if 'PROBE_OUT' not in t:
    t=t.replace('''        println!("MARKCOUNT={}", marks);
        let _ = std::io::stdout().flush();''','''        println!("MARKCOUNT={}", marks);
        let _ = std::io::stdout().flush();
        if let Ok(path) = std::env::var("PROBE_OUT") {
            let _ = std::fs::write(&path, format!("BYTES={} MARKCOUNT={}", total, marks));
        }''')
    t=t.replace('''    println!("MARKCOUNT={}", marks);
}''','''    println!("MARKCOUNT={}", marks);
    if let Ok(path) = std::env::var("PROBE_OUT") {
        let _ = std::fs::write(&path, format!("BYTES={} MARKCOUNT={}", total, marks));
    }
}''')
io.open(q,'w',encoding='utf-8').write(t)
print('probe out', 'PROBE_OUT' in t)
