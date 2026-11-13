#!/usr/bin/env python3
"""Expose/遮挡恢复取证辅助（阶段 4B-fix1 任务 C）。

用法：expose_overlay.py <目标窗口 id（十六进制，不带 0x）>

只操作**本脚本自己创建**的窗口：创建一个与目标同区域的覆盖窗、短暂显示后销毁，
使目标窗口收到系统级 Expose（被遮挡后重新暴露）。不使用 xdotool/wmctrl，不改
其他窗口状态，也不触碰窗口管理器。
"""

import sys
import time

from Xlib import X, display


def main() -> int:
    if len(sys.argv) < 2:
        print("用法: expose_overlay.py <win_id_hex>", file=sys.stderr)
        return 2
    target = int(sys.argv[1], 16)
    d = display.Display()
    screen = d.screen()
    try:
        geom = d.create_resource_object("window", target).get_geometry()
        x, y, w, h = geom.x, geom.y, max(geom.width, 10), max(geom.height, 10)
    except Exception as e:  # noqa: BLE001 - 取几何失败时用默认区域
        print(f"get_geometry 失败（{e}），使用默认区域", file=sys.stderr)
        x, y, w, h = 0, 0, 200, 100
    win = screen.root.create_window(
        x, y, w, h, 0, screen.root_depth, X.InputOutput, X.CopyFromParent
    )
    win.set_wm_name("expose-overlay")
    win.map()
    win.configure(stack_mode=X.Above)
    d.sync()
    time.sleep(0.5)
    win.destroy()
    d.sync()
    d.close()
    print("overlay 已销毁，目标窗口应收到 Expose")
    return 0


if __name__ == "__main__":
    sys.exit(main())
