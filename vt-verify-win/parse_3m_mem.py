import csv, glob, re, statistics, sys
sys.stdout.reconfigure(encoding='utf-8')
D = r'C:\dev\tty\vt-verify-win\logs\3m-mem'

def curve(tag):
    rows = []
    for r in csv.DictReader(open(f'{D}\\{tag}.csv', encoding='ascii')):
        if r['ws_mb'] in ('dead', 'exited', ''):
            continue
        rows.append((float(r['t_ms']), float(r['ws_mb']), float(r['priv_mb'])))
    return rows

def perf(tag):
    txt = open(f'{D}\\{tag}.err', 'rb').read().decode('utf-8', 'replace')
    out = {}
    for k in ('first_frame_ms', 'bg_fonts_ms', 'cpu_renderer_ready_ms', 'fontdb_ms'):
        m = re.findall(rf'\[perf\] {k}=([\d.]+)', txt)
        if m:
            out[k] = float(m[0])
    out['faces'] = txt.count('font_steps_us')
    m = re.search(r'bg_fonts_ms=\d+ count=(\d+)', txt)
    out['bg_count'] = int(m.group(1)) if m else -1
    return out

def summarize(tag):
    rows = curve(tag)
    if not rows:
        return None
    peak = max(rows, key=lambda r: r[1])
    plateau = statistics.median([r[1] for r in rows[-10:]])
    plat_priv = statistics.median([r[2] for r in rows[-10:]])
    p = perf(tag)
    # 爬升段：从首次达到 plateau*0.55 到最后一次低于 peak-15 的点
    thr = plateau * 0.55
    rise = [t for t, w, _ in rows if w >= thr]
    rise_start = rise[0] if rise else None
    rise_end = peak[0]
    return dict(tag=tag, n=len(rows), first=rows[0], peak=peak, plateau=plateau,
                plat_priv=plat_priv, rise_start=rise_start, rise_end=rise_end, perf=p)

if __name__ == '__main__':
    import sys
    tags = sys.argv[1:] or ['fine-A1']
    print('| 运行 | 样本 | 首点 t/ws | 峰值 ws@t | 平台 ws/priv | 爬升段 | first_frame | bg_fonts | 面数 | bg_count |')
    print('|---|---|---|---|---|---|---|---|---|---|')
    for tag in tags:
        s = summarize(tag)
        if not s:
            print(f'| {tag} | (无数据) | | | | | | | | |')
            continue
        pf = s['perf']
        print('| {0} | {1} | {2:.0f}ms/{3:.0f}MB | **{4:.0f}MB@{5:.0f}ms** | {6:.0f}/{7:.0f}MB | {8}→{9:.0f}ms | {10} | {11} | {12} | {13} |'.format(
            s['tag'], s['n'], s['first'][0], s['first'][1], s['peak'][1], s['peak'][0],
            s['plateau'], s['plat_priv'],
            f"{s['rise_start']:.0f}ms" if s['rise_start'] is not None else '-', s['rise_end'],
            pf.get('first_frame_ms', '-'), pf.get('bg_fonts_ms', '-'), pf['faces'], pf['bg_count']))
