# PHASE3T-WIN：合并 4V / 退出检测复验 / 拖选与标题 / 空闲唤醒 / 截图差异归因 / 硬编码修复 / RC1

- 日期：2026-11-17（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`，origin = `ssh://ali:8022/srv/git/tty.git`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 采集方式：`APP_PERF_FRAMES=1` + 直接句柄重定向；进程只结束本任务 `Start-Process -PassThru` 启动、命令行核对过的 `app.exe`/`shim.exe`。

## 1. t1 合并 4V

| 项目 | 结果 |
|---|---|
| `phase4v-done` | `d71879b8499cd902cb74168c09667d016e1c9884`；`git cat-file -t` = commit（**轻量 tag，无 `^{}`**，解引用同值） |
| 合并 | `--no-ff`，合并提交 `44c1ac8`，0 冲突 |
| 含 4U 的 rustfmt | 是（`7525f46 style: cargo fmt（4V t2 新增代码）`、4U 交付提交 `e5478cd`） |
| 关键修复到场 | `finish_wake` 唯一出口（`crates/app/src/lib.rs:2320`）、诊断开关 `APP_FORCE_LIVENESS_POLL` |

## 2. t2 退出检测 A/B/C′（本轮重点，已验证事实）

构建：A=`phase4r-done`、B=`phase4s-done`、C′=`phase4v-done`，各自独立 worktree + 独立 `CARGO_TARGET_DIR`（仓库外 `E:\dev\3s`、`E:\dev\3t`）。
驱动方式：`terminal.shell.args` 未接线（见 §8 请求），用无依赖 `shim.exe` 作 shell，命令内先写 `marker.txt`（证明命令确实执行、注入生效），
退出前写 `done.txt`（子 shell 退出时刻的代理），超时 15 s。判定阈值按任务书：子 shell 退出后 **≤5 s** 关闭窗口。

| 构建 | shell | 组合数 | 退出率 | 子 shell 退出→app 退出（ms） |
|---|---|---|---|---|
| A | cmd | 6（sys/bundle × 3） | 6/6 | 78–108 |
| A | pwsh | 6 | 6/6 | 229–434 |
| B | cmd | 6 | **0/6** | — （15 s 内不关闭，CPU ≈1.06 s） |
| B | pwsh | 6 | **0/6** | — （CPU ≈1.02–1.20 s） |
| C′ | cmd | 6 | **6/6** | **1711–1831** |
| C′ | pwsh | 6 | **6/6** | **1456–1977** |

- A 另跑 8 次补测（sys/bundle × cmd × 4）：8/8 关闭、110–128 ms；**A 全程 20 次中 1 次未关闭（5%）**，
  与已知的"ConPTY 主端 reader EOF 延迟"现象一致（`docs/WIN-NOTES.md` 记录过约 83 s 的极端值），非本轮新增缺陷。
- "直接结束子 shell"（只杀本测试 `shim.exe`，按命令行唯一目录匹配）：

| 构建 | 关闭率 | 杀 shell→app 退出 |
|---|---|---|
| A | 4/4 | 101–150 ms |
| B | 1/4（另 1 次 8965 ms） | — |
| C′ | 4/4 | 1125–1400 ms |

- 结论：**A 关、B 不关、C′ 关（≤5 s）与预期一致**，4V 的 `finish_wake` 修复在 Windows 上生效（1 s 存活召回 → 实测 1.5–2.0 s 关闭）。
- 原始数据：`vt-verify-win/logs/3t/out-t2b.csv`、`out-kill.csv`。

## 3. t3 拖选自动滚动 + OSC 标题

- **OSC 0 标题（已验证事实）**：shell 写 `ESC]0;3T-OSC-TITLE BEL` 后，窗口标题在 **12 ms** 内由可执行文件路径变为 `3T-OSC-TITLE`
  （cpu：marker 1812 ms → 标题 1824 ms；WARP：1994 → 2006 ms）。标题用 `EnumWindows + GetWindowText` 读自己进程的窗口。
- **拖选自动滚动（注入已生效，结果为分方法记录）**：
  - 注入生效证据：滚轮 6 格后截图变化（`wheel_changed=True`）、按下并拖到窗口外后内容变化；窗口前置 `fg=True`。
  - cpu 后端（PrintWindow 捕获）：拖到下边缘外 2 s **继续滚动=True**，松开后 1.5 s **停止=True**；上边缘外同样 True/True。
  - WARP 后端：PrintWindow 捕获下两方向均为 False（DX12 合成下 PrintWindow 疑似取到陈旧帧），改用屏幕捕获
    （`Graphics.CopyFromScreen`）后两方向 **继续滚动=True**、松开 **停止=True**。
  - 同一后端在"另一种捕获法"下出现过 False（cpu 用屏幕捕获时下边缘 False、上边缘松开后 False），因此
    **auto-scroll 的逐后端结论依赖捕获方式**；能确证的是：两种后端都在至少一种捕获方式下观测到"按住持续滚动 + 松开停止"。
    推翻条件：固定捕获方式后重复 5 次仍出现相反结果。

## 4. t4 空闲唤醒（C′，30 s）

输出 40 行后静止，`APP_PERF_FRAMES=1`，直接句柄重定向，采样 38 s：

| 后端 | 5 秒统计行 | 每行 `wake_timer` | 折算唤醒率 |
|---|---|---|---|
| cpu | 9（第 1 条为启动） | 5（启动行 0） | ≈1 次/秒 |
| WARP（DX12，`APP_GPU_ALLOW_SOFTWARE=1`） | 8 | 5 | ≈1 次/秒 |

- 与任务书预期一致（存活检查每秒一次 → 统计行每 5 秒一条）。4S 的"空闲 0 唤醒"在 4V 之后变为"1 次/秒"，属**预期变化**，不是回归。

## 5. t5 截图差异分布（cpu vs WARP，960×480）

同内容（固定文本 4 行 + 40 行滚动 + 中文行 + `ESC[44m` 段），同尺寸 960×480（3Q 的 960 宽；4S 的 800 宽不适用）。
差异按通道最大差统计：

| 指标 | 数值 | 占全图 |
|---|---|---|
| 差异像素（>0） | 4748 | **1.0304%** |
| 差异像素（>32） | 4716 | **1.0234%** |
| 右缘滚动条列（12 px 物理宽） | 9 px | 0.1562%（列内）/ 列面积占全图 1.25% |
| 列外·字形区 | 4739 | 1.0284% |
| 列外·背景区 | 0 | 0% |

- 差异包围盒 `(33,11)-(950,476)`；连通区域 ≥10 px 共 165 个；前 5 热点：
  474 px `(134,11)-(207,22)`、48 px `(608,166)-(622,172)`、39 px `(434,37)-(447,44)`、35 px `(649,274)-(655,284)`、35 px `(521,386)-(527,396)`。
- 滚动条：两图的右缘暗像素均为 450 行、跨度 (0,479)，拇指几何一致（列内仅 9 px 差异）。
- **1.46%/1.03% 的真实归因**：差异集中在**字形光栅**（列外字形区 4739 px ≈ 1.03%），滚动条列只占 9 px。
  4S §3 把 3Q 的 1.46% 归因为"滚动条列占 1.5% 宽"的工程判断，在 960 宽、WARP 与 CPU 的对比中**不成立**
  （滚动条列面积确为 1.25%，但其差异仅 0.156%）。差异来源待查（工程判断：DX12/WARP 的字形混合或 gamma 路径与 CPU 不同；推翻条件：换真实 GPU 后字形区差异显著下降）。
- 原始统计：`vt-verify-win/logs/3t/t5-diff.txt`；截图 `E:\dev\3t\t5-cpu.png`、`t5-gpu.png`（仓库外，未入库）。

## 6. t6 硬编码修复（shared 改动按任务书例外）

| 改动 | 提交 | 说明 |
|---|---|---|
| `crates/pty/src/windows.rs` + `crates/pty/tests/pty_windows.rs` 纯格式化 | `806f053 shared: 3T t6 cargo fmt…` | `rustfmt --check` 现为零 diff |
| `crates/pty/tests/pty_windows.rs` 去硬编码 | `51aa45c shared: 3T t6 去除 pty 测试的 C:\dev\tty 硬编码…` | 改为 `env!("CARGO_MANIFEST_DIR")` 上溯两级得仓库根 |
| `scripts/windows/e2e/wb2_lib.ps1` 的 `$ROOT` | `c34d7f6 3T t6：wb2_lib.ps1 的 $ROOT 改为由 $PSScriptRoot 推导…` | `Resolve-Path (Join-Path $PSScriptRoot '..\..\..')` |

验证：`cargo test -p pty --test pty_windows t6_probe` 通过（0.92 s）；从 `E:\dev\tty` 与 `C:\` 两个 cwd 加载 `wb2_lib.ps1`，
`$ROOT` 均解析为 `E:\dev\tty`；pty 测试全量 **22 passed / 0 failed**（含 wrap / ESC[7m 相关探针）。

## 7. t7 候选部署包 RC1（不是最终包）

```
zip: vt-verify-win/tty-win-x64.zip  bytes=6 604 572  sha256=4b7a720cf82f9f55b8d2f2fbd365ce4114025fdc175dd2a7b0e3f0f4f8181484
dist: vt-verify-win/dist/（app.exe, conpty.dll, OpenConsole.exe, config.example.toml,
      LICENSE-ConPTY.txt, NOTICE.md, README.md, MANIFEST.txt）
MANIFEST 首行: "# conpty: OpenConsole.exe FileVersion 1.25.2611.09001"，随后逐文件 bytes+sha256
app.exe  18 098 452  90b58d3f2c917f62c6009ab0354e716510c2b61d59a9075c381487aa9cf5f2b5
conpty.dll  119 608  e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492
OpenConsole.exe  1 089 336  b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775
```

- 生成命令：`powershell -File scripts/windows/package.ps1 -ConptyDir <kit v4> -NoticePath <kit v4>\NOTICE.md`
  （默认路径会走 `fetch-conpty.ps1` 联网，本机网络不可达 → 复用已核验的同 sha256 文件）。
- 解压与运行（已验证事实）：解压到 `%TEMP%\3T 部署 测试\dist`（含空格与中文），从 `C:\` 作为 cwd 启动，
  窗口 960×480；进程树里出现**来自该目录**的 `OpenConsole.exe`（bundled_OpenConsole=1），说明随附 ConPTY 生效；
  运行前后系统 `OpenConsole.exe` 集合不变（`10212,11780`，均为 WezTerm 自带，非本次启动）。
- 折行 / `ESC[7m` 的字节探针：由 `crates/pty/tests/pty_windows.rs` 的 ConPTY 探针覆盖，全量 22/22 通过
  （含 `conpty_pair_detection_needs_both_files`、Ctrl-C 系列、cwd 探针）。
- 日志：`vt-verify-win/logs/3t/t7-dist.log`。

## 8. 未解决问题与请求栏

1. **`terminal.shell.args` 仍未接线**（`crates/config` 解析但不传给 PTY spawn）：本任务只能用 `shim.exe` 绕行；影响所有"配置里指定命令参数"的场景。请求：接线或文档明示（crates 改动需授权）。
2. **A 构建 5%（1/20）不关闭**：与 ConPTY reader EOF 延迟一致，非 4V 引入；如需确定性，应把 A 也纳入存活检查（属产品决策，未改）。
3. **WARP 下 PrintWindow 取帧疑似陈旧**：t3 中同后端换捕获法结果不同；t5 的差异归因只依赖静态帧，未受影响。请求：给 GPU 后端提供稳定的帧捕获/对比手段（debug 钩子）。
4. **4S §3 的 1.46% 归因被本报告 §5 推翻**（960 宽、WARP vs CPU）：差异在字形区。请求：更新 4S 文档或补真实 GPU 数据。
5. `wb2_lib.ps1` 的 `$ROOT` 已修，但其它 Windows 脚本未逐一体检（本任务只要求该文件）。

## 9. 手动清单（≤3）

1. 有真实 GPU 的 Windows 机器上重跑 t5（同一内容、960 宽），确认字形区差异是否缩小。
2. 交互式桌面下用固定捕获方式复测 t3 的 auto-scroll 5 次，确认"按住滚动/松开停止"稳定。
3. 用 RC1 包在干净机器上跑一次 `run-tonight.ps1 -Quick`，确认随附 ConPTY 与系统 conhost 两种部署都正常。

## 10. 交付

- 提交：`44c1ac8`（合并）、`806f053`/`51aa45c`（shared）、`c34d7f6`（脚本）、本报告与 `docs/DECISIONS.md` 追加。
- 未改 `crates/` 以外的共用代码；二进制与 zip 未入库（`vt-verify-win/` 被忽略）。
