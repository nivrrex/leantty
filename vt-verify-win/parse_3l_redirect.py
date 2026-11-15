import glob, re, statistics, sys
sys.stdout.reconfigure(encoding='utf-8')

def parse(files):
    steps, groups, frames = [], [], []
    for f in files:
        txt = open(f, 'rb').read().decode('utf-8', errors='replace')
        for line in txt.splitlines():
            if 'font_steps_us' in line:
                d = dict(re.findall(r"(\w+)=('[^']*'|\S+)", line))
                rec = {'family': d['family'].strip("'"), 'style': d['style'].strip("'"),
                       'parse_us': int(d['parse_us']), 'clone_us': int(d['clone_us']),
                       'read_us': int(d['read_us']), 'bytes': int(d['bytes'])}
                steps.append(rec)
            elif 'font_group_us' in line:
                d = dict(re.findall(r"(\w+)=('[^']*'|\S+)", line))
                rec = {'group': d['group'], 'family': d['family'].strip("'"), 'style': d['style'].strip("'"),
                       'pre_load_us': int(d['pre_load_us']), 'load_us': int(d['load_us']),
                       'total_us': int(d['total_us'])}
                groups.append(rec)
            elif 'first_frame_ms=' in line:
                frames.append(float(re.search(r'first_frame_ms=([0-9.]+)', line).group(1)))
    return steps, groups, frames

PS = sorted(glob.glob(r'C:\dev\tty\vt-verify-win\logs\3l-font\A-*.err'))
BH = sorted(glob.glob(r'C:\dev\tty\vt-verify-win\logs\3l-font\bashA-*.err'))
print('## stderr 重定向对照（同一 release 二进制，默认配置，各 5 次）\n')
print('| 重定向方式 | first_frame_ms 中位 / 最小 | 组 | pre_load 中位 (ms) | load 中位 (ms) | total 中位 (ms) | steps 中位 (ms) | total−steps (ms) |')
print('|---|---|---|---|---|---|---|---|')
for name, files in (('PowerShell `Start-Process -RedirectStandardError`', PS), ('bash `2>file`', BH)):
    steps, groups, frames = parse(files)
    seen = []
    for g in groups:
        k = (g['group'], g['family'], g['style'])
        if k in seen: continue
        seen.append(k)
        rows = [x for x in groups if (x['group'], x['family'], x['style']) == k]
        st = [s['parse_us'] + s['clone_us'] for s in steps if s['family'] == g['family'] and s['style'] == g['style']]
        t = statistics.median([r['total_us'] for r in rows]) / 1000
        pv = statistics.median([r['pre_load_us'] for r in rows]) / 1000
        ld = statistics.median([r['load_us'] for r in rows]) / 1000
        sv = statistics.median(st) / 1000 if st else float('nan')
        print(f"| {name if k == seen[0] else ''} | {statistics.median(frames):.0f} / {min(frames):.0f} | {k[0]} {k[1][:7]} {k[2][:6]} | {pv:.2f} | {ld:.0f} | {t:.0f} | {sv:.1f} | {t - sv:.1f} |")
