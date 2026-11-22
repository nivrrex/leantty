import sys
CR, LF = b"\r", b"\n"
START, END = b"\x1b[200~", b"\x1b[201~"
d = open(sys.argv[1], 'rb').read()
src = open(sys.argv[2], 'rb').read()
body = d[len(START):-len(END)] if d.startswith(START) and d.endswith(END) else b""
print(f"{sys.argv[3]}: 收到 {len(d)} B | 200~ x{d.count(START)} 201~ x{d.count(END)} | "
      f"CR {d.count(CR)} LF {d.count(LF)} CRCR {d.count(CR+CR)} | 正文 {len(body)} B | 源 {len(src)} B")
