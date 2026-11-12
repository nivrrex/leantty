import io
q='C:/dev/tty/scripts/windows/e2e/run_2c_win.ps1'
t=io.open(q,encoding='utf-8',newline='').read().replace('\r\n','\n')

# c1: select through the B3 row, not B1
t=t.replace("""    $rB = (Find-RowRetry $a.dump 'B1' 6).row""","""    $rB = (Find-RowRetry $a.dump 'B3' 6).row""")

# c2a: pick two rows that actually contain text
t=t.replace("""    $g2 = Dump-Grid $a.dump
    # select the first two visible text rows
    $p1 = Cell-XY $geom $g2.cols $g2.rows 1 0
    $p2 = Cell-XY $geom $g2.cols $g2.rows 2 ($g2.cols - 4)""","""    $g2 = Dump-Grid $a.dump
    $rowA = 1
    for ($k = 1; $k -lt $g2.rows - 1; $k++) {
        if ($g2.grid[$k].Contains('L') -and $g2.grid[$k + 1].Contains('L')) { $rowA = $k; break }
    }
    # prime click so the first press is not consumed by activation
    $prime = Cell-XY $geom $g2.cols $g2.rows ([math]::Min($rowA + 3, $g2.rows - 1)) 3
    Click-Left $prime.x $prime.y
    Start-Sleep -Milliseconds 350
    $p1 = Cell-XY $geom $g2.cols $g2.rows $rowA 0
    $p2 = Cell-XY $geom $g2.cols $g2.rows ($rowA + 1) ($g2.cols - 4)""")

# c2b: prime + drag inside the last rows, pointer held just inside the bottom band
t=t.replace("""    $p1 = Cell-XY $geom $g2.cols $g2.rows 1 0
    $bottom = Cell-XY $geom $g2.cols $g2.rows ($g2.rows - 1) 10
    [System.Windows.Forms.Clipboard]::Clear()
    Move-To $p1.x $p1.y""","""    $p1 = Cell-XY $geom $g2.cols $g2.rows 1 0
    $bottom = Cell-XY $geom $g2.cols $g2.rows ($g2.rows - 1) 10
    [System.Windows.Forms.Clipboard]::Clear()
    Click-Left $p1.x $p1.y
    Start-Sleep -Milliseconds 350
    Move-To $p1.x $p1.y""")

# c5: prime click before the paste click
t=t.replace("""    $c = Cell-XY $geom $g0.cols $g0.rows 1 40
    Click-Right $c.x $c.y
    # type the marker five times while the paste is still draining""","""    $c = Cell-XY $geom $g0.cols $g0.rows 1 40
    Click-Left $c.x $c.y
    Start-Sleep -Milliseconds 400
    Click-Right $c.x $c.y
    # type the marker five times while the paste is still draining""")
io.open(q,'w',encoding='utf-8',newline='\n').write(t)
print('c1 B3', "Find-RowRetry $a.dump 'B3'" in t)
print('c2a prime', 'pick two rows that actually contain text' in t)
print('c5 prime', 'Click-Left $c.x $c.y' in t.split("# ---------- c5")[1][:600] if "# ---------- c5" in t else False)
