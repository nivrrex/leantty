import csv, glob, re, statistics, sys
sys.stdout.reconfigure(encoding='utf-8')
D = r'C:\dev\tty\vt-verify-win\logs\3l-mem'

def load_csv(tag, i):
    rows = []
    for r in csv.DictReader(open(f'{D}\\{tag}-{i}.csv', encoding='ascii')):
        if r['ws_mb'] in ('dead', ''):
            continue
        rows.append((float(r['t_ms']), float(r['ws_mb']), float(r['priv_mb'])))
    return rows

def jumps(rows, thresh_mb=20.0):
    out = []
    for a, b in zip(rows, rows[1:]):
        d = b[1] - a[1]
        if abs(d) >= thresh_mb:
            out.append((b[0], a[1], b[1], d))
    return out

def perf_marks(tag, i):
    txt = open(f'{D}\\{tag}-{i}.err', 'rb').read().decode('utf-8', errors='replace')
    marks = []
    for m in re.finditer(r'\[perf\] (\w+)=([\d.]+) since_start=([\d.]+)', txt):
        marks.append((m.group(1), float(m.group(3))))
    for m in re.finditer(r'\[perf\] (first_frame_ms|bg_fonts_ms)=([\d.]+)', txt):
        marks.append((m.group(1), float(m.group(2))))
    return marks

for tag in ('A', 'B'):
    print(f'\n## 配置 {tag}')
    allrows = []
    for i in (1, 2, 3):
        rows = load_csv(tag, i)
        allrows.append(rows)
        js = jumps(rows)
        marks = perf_marks(tag, i)
        print(f'\n### {tag}-{i}：采样 {len(rows)} 点，5 ms 内最大 {max(r[1] for r in rows):.0f} MB')
        if rows:
            print(f'  首点 t={rows[0][0]:.0f}ms ws={rows[0][1]:.0f}；5s 时 ws={[r[1] for r in rows if abs(r[0]-5000) < 60][:1]}；60s 时 ws={rows[-1][1]:.0f}/{rows[-1][2]:.0f}')
        print('  跳升（|ΔWS| ≥ 20MB）：')
        for t, a, b, d in js:
            print(f'    t={t:.0f}ms  {a:.0f} -> {b:.0f} MB  (Δ {d:+.0f})')
        print('  perf 锚点：' + ', '.join(f'{k}@{v:.0f}ms' for k, v in marks if k in ('first_frame_ms', 'bg_fonts_ms', 'fontdb_ms', 'font_group_begin_ms')))
    if allrows:
        print('\n  三次运行 ws 采样中位（按时间刻度对齐）:')
        print('  | t(s) | A/B 中位 ws (MB) |')
        print('  |---|---|')
        for t in (0.1, 0.5, 1, 1.5, 2, 2.5, 3, 4, 5, 10, 20, 30, 40, 50, 60):
            vals = []
            for rows in allrows:
                if not rows:
                    continue
                best = min(rows, key=lambda r: abs(r[0] - t * 1000))
                if abs(best[0] - t * 1000) < 400:
                    vals.append(best[1])
            if vals:
                print(f'  | {t} | {statistics.median(vals):.0f} |')
