import sys, time
data = open(sys.argv[1], 'rb').read()
sys.stdout.buffer.write(data)
sys.stdout.buffer.flush()
time.sleep(600)
