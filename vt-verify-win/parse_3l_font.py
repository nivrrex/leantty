import glob, re, statistics, sys
sys.stdout.reconfigure(encoding='utf-8')

STEP_KEYS = ['query_us','face_us','meta_us','read_us','settings_us','parse_us','clone_us','insert_us','steps_us']
GROUP_KEYS = ['pre_load_us','load_us','tail_us','total_us']

def parse(files):
    steps, groups, frames = [], [], []
    for f in files:
        txt = open(f, 'rb').read().decode('utf-8', errors='replace')
        for line in txt.splitlines():
            if 'font_steps_us' in line:
                d = dict(re.findall(r"(\w+)=('[^']*'|\S+)", line))
                rec = {'file': f, 'family': d["family"].strip("'"), 'style': d['style'].strip("'"),
                       'kind': d['kind'], 'bytes': int(d['bytes']), 'ttc': d['ttc']}
                for k in STEP_KEYS:
                    rec[k] = int(d[k])
                steps.append(rec)
            elif 'font_group_us' in line:
                d = dict(re.findall(r"(\w+)=('[^']*'|\S+)", line))
                rec = {'file': f, 'group': d['group'], 'family': d["family"].strip("'"),
                       'style': d['style'].strip("'"), 'fallback': d['fallback']}
                for k in GROUP_KEYS:
                    rec[k] = int(d[k])
                groups.append(rec)
            elif 'first_frame_ms=' in line:
                frames.append(float(re.search(r'first_frame_ms=([0-9.]+)', line).group(1)))
    return steps, groups, frames

def agg(rows, key):
    vals = [r[key] for r in rows]
    return statistics.median(vals), min(vals)

def table(title, steps, groups, only_family=None):
    print(f'\n### {title}')
    sel = [s for s in steps if only_family is None or s['family'] == only_family]
    if not sel:
        print('(无 font_steps_us 行)')
        return
    hdr = '| family | style | kind | bytes | ttc | ' + ' | '.join(k.replace('_us','') for k in STEP_KEYS) + ' |'
    print(hdr)
    print('|' + '---|' * (5 + len(STEP_KEYS)))
    seen = []
    for s in sel:
        k = (s['family'], s['style'])
        if k in seen: continue
        seen.append(k)
        rows = [x for x in sel if (x['family'], x['style']) == k]
        cells = [f"{s['family']}", f"{s['style']}", s['kind'], f"{s['bytes']:,}".replace(',', ' '), s['ttc']]
        for key in STEP_KEYS:
            m, mn = agg(rows, key)
            cells.append(f'{m:.0f}/{mn:.0f}')
        print('| ' + ' | '.join(cells) + ' |')
    # group 表
    print('\n| group | family | style | fallback | ' + ' | '.join(k.replace('_us','') for k in GROUP_KEYS) + ' | (total − steps) |')
    print('|' + '---|' * (4 + len(GROUP_KEYS) + 1))
    seen_g = []
    for g in groups:
        gk = (g['group'], g['family'], g['style'])
        if gk in seen_g: continue
        seen_g.append(gk)
        rows = [x for x in groups if x['group'] == g['group'] and x['family'] == g['family'] and x['style'] == g['style']]
        cells = [g['group'], g['family'], g['style'], g['fallback']]
        for key in GROUP_KEYS:
            m, mn = agg(rows, key)
            cells.append(f'{m:.0f}/{mn:.0f}')
        # total - steps：把同组多次运行的 steps 中位取来比
        st = [s['steps_us'] for s in sel if s['family'] == g['family'] and s['style'] == g['style']]
        if st and rows:
            diff = statistics.median([r['total_us'] for r in rows]) - statistics.median(st)
            cells.append(f'{diff/1000:.1f} ms')
        else:
            cells.append('n/a（无 steps 行：缓存命中）')
        print('| ' + ' | '.join(cells) + ' |')

files = sorted(glob.glob(r'C:\dev\tty\vt-verify-win\logs\3l-font\bashA-*.err'))
steps, groups, frames = parse(files)
print(f'## 主字体面（bash 重定向，{len(files)} 次运行）first_frame_ms 中位/最小 = {statistics.median(frames):.0f}/{min(frames):.0f}')
table('主字体面（Consolas，默认配置）', steps, groups)
print(f"\n解析的 face 次数（font_steps_us 行总数）={len(steps)}，{' '.join(sorted({r['bytes'] for r in steps} and [str(r['bytes']) for r in steps]))} 字节集合")

files = sorted(glob.glob(r'C:\dev\tty\vt-verify-win\logs\3l-font\ttc-*.err'))
steps, groups, frames = parse(files)
print(f'\n## 回退字体面（Microsoft YaHei / ttc，{len(files)} 次运行）first_frame_ms 中位/最小 = {statistics.median(frames):.0f}/{min(frames):.0f}')
table('回退字体面（Microsoft YaHei，ttc 集合）', steps, groups)
print(f'\n解析的 face 次数（font_steps_us 行总数）={len(steps)}')

