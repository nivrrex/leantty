# 性能对照：leantty(Rust) vs leantty-zig vs alacritty 0.17.0

采集日期：2026-11-22 ｜ 机器：Devuan 5、Xvfb `:99`（1920×1080）+ openbox ｜ 数据：10 MB 文本（10 万行 × 100 字符）

## 方法

- 子进程做被测终端里的 `-e` 命令：`s=$(date +%s%N); cat data.txt; e=$(date +%s%N)`，把耗时写文件。
  `cat` 写的是 pty，终端消费不完就会阻塞，所以该耗时就是"终端把 10 MB 吃进去"的墙上时间。
- 同时采集：窗口出现时间（`xdotool search --class`）、进程累计 CPU（`/proc/PID/stat` 的 utime+stime）、
  峰值 RSS（`VmHWM`）、空闲 3 秒的 CPU 增量。每项跑 3 轮取中位。
- **渲染后端**：Rust 分别测 `cpu` / `gpu`（Vulkan，适配器 `NVIDIA GeForce GT 710`，DiscreteGpu）/
  `gpu`+lavapipe（`llvmpipe`，Cpu 类型）；Zig 用其默认 `auto`；alacritty 0.17 只有 OpenGL，
  在 Xvfb 下 `glxinfo` 显示 `llvmpipe`（软件渲染）。

## 结果（3 轮中位）

| 实现 | 后端 | 窗口出现 ms | 10 MB 耗时 ms | 进程 CPU ms | 峰值 RSS MB | 空闲 CPU（3s）|
|---|---|---|---|---|---|---|
| leantty (Rust 0.2.10) | cpu | 147 | 331 | 530 | 57 | 0 |
| leantty (Rust 0.2.10) | gpu (Vulkan, GT 710) | 138 | 592 | 1050 | 223 | 0 |
| leantty (Rust 0.2.10) | gpu (lavapipe) | 150 | 398 | 810 | 139 | 0 |
| leantty-zig 0.1.3 | auto（GLX） | 94 | 182 | 210 | 32 | 0 |
| alacritty 0.17.0 | GL（llvmpipe） | 99 | 336 | 1790 | 162 | 0 |

按 10 MB 折算的 CPU 成本：Zig **21 ms/MB**、Rust-cpu **53**、Rust-gpu(lavapipe) **81**、
Rust-gpu(硬件) **105**、alacritty **179**。

## 观察

1. **Zig 版三项全优**：墙上时间比 alacritty 快约 1.85×、CPU 低 8.5×、RSS 只有 32 MB；启动也最快（94 ms）。
   它的绘制路径是"字形直画 + XShm"，没有独立的 GPU 后端与图集。
2. **Rust 版 CPU 明显低于 alacritty**（530 vs 1790 ms）：我们的 CPU 光栅器 + 脏行增量，比 alacritty
   在这台机器上的软件 GL 便宜约 3.4×；墙上时间与 alacritty 持平（331 vs 336 ms）。
3. **Rust 的 GPU 后端在这台机器上并不划算**：真机 `GT 710` 上 10 MB 用了 592 ms（比 CPU 慢 1.8×），
   RSS 223 MB；换成 lavapipe 也有 398 ms。原因是每帧 present 约 19 ms（FIFO 交换链 + 弱 GPU），
   帧节流成为瓶颈——与 0.2.10 新增的 `present_ms_max` 观测一致，不是本次改动的回归。
4. **启动**：Zig 94 ms < alacritty 99 ms < Rust 138–150 ms。Rust 多出来的是 fontconfig/字体覆盖库
   （`[perf] fontset_load_ms` + 首次缺字时的 `font_coverage_db_ready`）。
5. **空闲**：三者空闲 3 秒 CPU 均为 0 ms（无定时器空转）。

## 限制

- 全部在 Xvfb 下测：alacritty 只能走软件 GL（llvmpipe），CPU 数字对它不利；真实桌面上硬件 GL 会
  便宜很多。Rust 的 GPU 数字则是**真机 Vulkan**（GT 710），条件比 alacritty 好。
- 未测滚动回看、超大粘贴、多窗口、真机 `:0`、Windows；10 MB 单次 `cat` 只反映"持续输出 + 短回滚"这一场景。
- 时间取 3 轮中位，未做统计显著性检验；同机同会话内波动约 ±10%。
