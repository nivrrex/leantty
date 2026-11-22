import os, sys, time, tty
out = sys.argv[1]
f = open(out, "wb")
os.write(1, b"\x1b[?2004h"); os.write(1, b"READY\r\n")
tty.setraw(0)
end = time.time() + float(sys.argv[2])
while time.time() < end:
    try:
        chunk = os.read(0, 4096)
    except OSError:
        break
    if not chunk:
        break
    f.write(chunk); f.flush()
f.close()
