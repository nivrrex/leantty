#!/usr/bin/env python3
"""按键编码探针（阶段 6A t2a，仅测试用）。

在 PTY 里以 raw 模式运行，先输出一组"终端查询/模式开启"序列（取自 pi 启动时的真实字节，
见 docs/PHASE6A-LINUX.md §1），记录终端对这些查询的应答，之后把 stdin 收到的每个字节
（hex）连同时间戳写入日志。

用法：
    python3 keyprobe.py --mode <mode> --log <path> [--settle 0.6]

mode 预设：
    legacy     不输出任何序列（纯 legacy 基线）
    kitty1     CSI > 1 u  + CSI ? u          （kitty flags=1，仅 disambiguate）
    kitty7     CSI > 7 u  + CSI ? u          （kitty flags=1|2|4，pi 启动时用的值）
    mok2       CSI > 4 ; 2 m + CSI ? 4 m     （xterm modifyOtherKeys level 2）
    pi         pi 启动序列的前缀（括号粘贴 + kitty 7 + DA1 + 隐藏光标）

日志格式（每行一条，便于 grep）：
    <t_ms> <dir> <hex>
其中 dir = OUT（探针输出给终端）、IN（探针从终端收到）。会话结束时写一行 END。
"""

import argparse
import os
import select
import sys
import time

PRESETS = {
    "legacy": b"",
    "kitty1": b"\x1b[>1u\x1b[?u",
    "kitty7": b"\x1b[>7u\x1b[?u",
    "mok2": b"\x1b[>4;2m\x1b[?4m",
    # pi 启动时的开头字节（实测顺序）：括号粘贴 → kitty 7 → 查询 → DA1
    "pi": b"\x1b[?2004h\x1b[>7u\x1b[?u\x1b[c",
}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", default="legacy", choices=sorted(PRESETS))
    ap.add_argument("--log", required=True)
    ap.add_argument("--settle", type=float, default=0.6, help="等终端应答的秒数")
    ap.add_argument(
        "--extra",
        default="",
        help="额外序列（hex 或 @file），发在预设之后；用于逐条查询",
    )
    args = ap.parse_args()

    log = open(args.log, "w", buffering=1)
    t0 = time.monotonic()

    def emit(direction: str, data: bytes) -> None:
        log.write(f"{int((time.monotonic() - t0) * 1000)} {direction} {data.hex()}\n")

    # raw 模式：不本地回显、不缓冲、不做信号字符处理
    import termios
    import tty

    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    tty.setraw(fd)

    try:
        seq = PRESETS[args.mode]
        if args.extra:
            extra = args.extra
            if extra.startswith("@"):
                extra = open(extra[1:], "rb").read().hex()
            seq += bytes.fromhex(extra)
        if seq:
            os.write(sys.stdout.fileno(), seq)
            emit("OUT", seq)

        # 收集终端应答（settle 窗口内）
        deadline = time.monotonic() + args.settle
        while time.monotonic() < deadline:
            r, _, _ = select.select([fd], [], [], 0.05)
            if fd in r:
                data = os.read(fd, 4096)
                if not data:
                    break
                emit("IN", data)

        # 之后持续记录按键字节，直到对端关闭或收到 Ctrl-D 之外的结束信号
        while True:
            r, _, _ = select.select([fd], [], [], 5.0)
            if fd not in r:
                log.write("IDLE\n")
                continue
            data = os.read(fd, 4096)
            if not data:
                log.write("END eof\n")
                break
            emit("IN", data)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        log.write("END exit\n")
        log.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
