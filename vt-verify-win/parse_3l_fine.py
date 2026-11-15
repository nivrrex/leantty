import csv, re, statistics, sys
sys.stdout.reconfigure(encoding='utf-8')
D = r'C:\dev\tty\vt-verify-win\logs\3l-mem'

def curve(tag):
    rows = [(float(r['t_ms']), float(r['ws_mb'])) for r in csv.DictReader(open(f'{D}\\fine-{tag}.csv', encoding='ascii'))
            if r['ws_mb'] not in ('exited', '')]
    return rows

def marks(tag):
    txt = open(f'{D}\\fine-{tag}.err', 'rb').read().decode('utf-8', errors='replace')
    m = {}
    for k in ('config_ms', 'fontdb_ms', 'cpu_groups_ms', 'cpu_renderer_ready_ms', 'window_ms', 'first_frame_ms', 'bg_fonts_ms'):
        mm = re.findall(rf'\[perf\] {k}=([\d.]+)', txt)
        if mm:
            m[k] = float(mm[0])
    return m, txt

print('## 内存曲线（20ms 采样，stderr 到文件 = 慢路径，绝对时间偏大；用同一次运行的 perf 锚点对齐）\n')
print('| 运行 | 首点 ws | 峰值 ws | 峰值时刻(自采样起) | 平台值(末点) | 上升区间 |')
print('|---|---|---|---|---|---|')
for tag in ('A1', 'A2', 'A3', 'B1', 'B2', 'B3'):
    rows = curve(tag)
    peak_t, peak_w = max(rows, key=lambda r: r[1])
    final = rows[-1][1]
    # 上升区间：从最后一次低于 final+5 之前的谷底到峰值
    rise_start = None
    for t, w in rows:
        if w < 60:
            rise_start = t
    print(f'| {tag} | {rows[0][1]:.0f} MB | **{peak_w:.0f} MB** | {peak_t:.0f} ms | {final:.0f} MB | {rise_start if rise_start else "-"} → {peak_t:.0f} ms |')

print('\n### 曲线采样点（A1，每 ~100ms 一个）\n')
rows = curve('A1')
print('| t(ms) | ws(MB) |')
print('|---|---|')
for t, w in rows:
    if int(t) % 100 < 21:
        print(f'| {t:.0f} | {w:.0f} |')

print('\n### perf 锚点（同一次运行，app 自 since_start）\n')
for tag in ('A1', 'B1'):
    m, _ = marks(tag)
    print(f'- {tag}: ' + ', '.join(f'{k}={v:.0f}ms' for k, v in m.items()))

print('\n### 对齐：把 perf 锚点换算到采样时间轴（偏移 = 首点 ws=进程创建后 ~1 个采样间隔）\n')
for tag in ('A1', 'A2', 'A3', 'B1', 'B2', 'B3'):
    rows = curve(tag)
    m, _ = marks(tag)
    # 进程创建时刻 ≈ 第一个 ws<10MB 的点
    birth = next((t for t, w in rows if w < 10), rows[0][0])
    line = [f'{tag}: 出生≈{birth:.0f}ms']
    for k in ('first_frame_ms', 'bg_fonts_ms'):
        if k in m:
            line.append(f'{k}@{birth + m[k]:.0f}ms')
    peak_t, peak_w = max(rows, key=lambda r: r[1])
    line.append(f'峰值 {peak_w:.0f}MB@{peak_t:.0f}ms')
    print('  ' + '  '.join(line))
