import ctypes, sys, time
from ctypes import wintypes

u32 = ctypes.WinDLL('user32', use_last_error=True)
WM_CHAR, WM_KEYDOWN, WM_KEYUP = 0x0102, 0x0100, 0x0101

def hwnd_for_pid(pid):
    found = []
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(h, l):
        p = wintypes.DWORD()
        u32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid and u32.IsWindowVisible(h):
            found.append(h)
        return True
    u32.EnumWindows(CB(cb), 0)
    return found[0] if found else None

pid = int(sys.argv[1]); text = sys.argv[2]
h = None
for _ in range(40):
    h = hwnd_for_pid(pid)
    if h: break
    time.sleep(0.5)
if not h:
    print('no hwnd'); sys.exit(3)
print('hwnd', h)
for ch in text:
    u32.PostMessageW(h, WM_CHAR, ord(ch), 1)
    time.sleep(0.02)
u32.PostMessageW(h, WM_KEYDOWN, 0x0D, 0x001C0001)
time.sleep(0.08)
u32.PostMessageW(h, WM_KEYUP, 0x0D, 0xC01C0001)
print('posted', len(text), 'chars')
