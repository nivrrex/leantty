# PHASE8C-WIN：0.2.1 发布包（第一部分）

- 日期：2026-11-18（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 说明：本文件分两部分记录；第一部分（0.2.1 发布包）在本节收口并打 tag，第二部分（吞吐诊断）见文末追加。

## E1 合并

`git ls-remote origin` 实际输出（节选）：

```
b7567b4d102ab8452dd92190ee3ce6a91f36b254	refs/heads/main
e09b015705cf8bc85dc2abdb41f674078c7d0aa7	refs/heads/win
bc5735c61b012a679c1a9491061ac17b8b7fe521	refs/tags/phase8a-done
```

- `phase8a-done` 与任务书 `bc5735c6…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- 合并冲突仅 `docs/DECISIONS.md`（双方文件尾追加）：按"双方保留"解决；合并提交 `71076d8`。
- `crates/` 相对 `phase7d-win-done`（`e09b015`）差异文件清单（全部随合并进入）：

| 文件 | 变化 | 内容 |
|---|---|---|
| `crates/app/src/lib.rs` | +82/−? | 选区交互接线（流式选区） |
| `crates/app/src/selection.rs` | +105 | `stream_row_range` 方向感知行范围 + `stream_tests` 3 例 |
| `crates/render/src/decide.rs` | +29 | 渲染判定相关 |
| `crates/render/tests/incremental.rs` | +19/−? | 增量渲染测试更新 |

## E2 重建与门禁

- `scripts/windows/build.ps1 … --release`：rc=0，`build.ps1 OK`。
- `--version` 原始输出：`leantty 0.2.1`（rc=0）。
- `cargo test --workspace`：**239 passed / 0 failed**。
- `cargo clippy --workspace --all-targets -- -D warnings`（windows-gnu 宿主）：**rc=0、0 error**。

## E3 回归（已验证事实）

| 项 | 结果 |
|---|---|
| `--version` | rc=0，`leantty 0.2.1` |
| `--help` | rc=0 |
| `--config-file` 缺值 / 不存在文件 | rc=2 / rc=1 |
| `-e` 缺程序 | rc=2 |
| `--working-directory "E:\dev\7b\dirs\with space" -e probe` | rc=0，子进程 `cwd=E:\dev\7b\dirs\with space` |
| `--working-directory E:\dev\8c\nodir -e probe` | **rc=1**，stderr `leantty: cannot use working directory 'E:\dev\8c\nodir': … (os error 2)` |
| 控制台释放（`-e cmd.exe /k …`） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0** |
| `-e probe`（子进程立刻退出） | leantty **574 ms** 后退出，rc=0，标记文件存在 |
| 默认配置启动（`--config-file config/leantty.windows.toml`） | `stderr_len=0`（无输出）；进程在 20 s 观察窗内仍运行（预期） |

## E4 选区单测（已验证事实，原始测试行）

```
test selection::auto_scroll_tests::auto_scroll_step_direction_and_clamp ... ok
test selection::offset_tests::selection_region_clamps_to_viewport ... ok
test selection::offset_tests::viewport_abs_roundtrip ... ok
test selection::stream_tests::single_row_and_zero_length ... ok
test selection::stream_tests::upward_drag_is_streaming ... ok
test selection::stream_tests::three_row_drag_is_streaming ... ok
```

- 鼠标注入本机不可用 → **拖选手感与真实鼠标点击"未测"**（仅覆盖纯函数/单测层）。

## E5 打包（leantty 0.2.1）

```
zip:  vt-verify-win/leantty-0.2.1-windows-x86_64.zip
size: 5 793 626 bytes
sha256: 25d47aa95f5d215b3e0d6f4bf41581266abeddd489c084c6be7237ba08cd9ec3
```

- 单顶层目录 `leantty-0.2.1-windows-x86_64/`，**10 条目**：`leantty.exe`（12 801 536 / `ddb005fd…`）、
  `conpty.dll`、`OpenConsole.exe`、`leantty.toml`、`leantty-portable.cmd`、`LICENSE`、
  `THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- **zip 内文档 vs 仓库逐文件哈希（raw 与去 CR 均相同，无 CRLF 差异）**：

| 文件 | raw sha256 | 去 CR |
|---|---|---|
| `leantty.toml` ↔ `config/leantty.windows.toml` | `8824111bd1fda252…` | 相同 |
| `README.md` | `9bd64fea263b0892…` | 相同 |
| `README.zh-CN.md` | `80bdcd63b9c992ab…` | 相同 |
| `CHANGELOG.md` | `22bcf202529c6458…` | 相同 |
| `LICENSE` | `45c88805fba0e684…` | 相同 |
| `THIRD-PARTY-NOTICES.md` | `cd28e4b0c6324e26…` | 相同 |

- 泄漏扫描（exe + zip 内 10 文件）：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root/` / `/home/` **全 0 命中**。

## 已知限制（第一部分）

1. 鼠标/键盘注入不可用：拖选、点击、IME 均**未测**（只覆盖单测与自动回归）。
2. UNC 可用目录未测（沿用 7B 记录）。
3. pwsh 7.6.6 更新横幅崩溃仍按 6D/6E 结论（与终端尺寸无关）。

## 手动项（≤3，仅无法自动覆盖的）

1. 真实鼠标点击与拖选手感（含单击不留标记、三行流式拖动）。
2. 解压 zip 后用 `leantty-portable.cmd` 启动并做真实输入法测试。
3. 在你机器上对 0.2.1 zip 重做 `ptime tree`，记录用时（用于与第二部分对照）。

---

# 第二部分：吞吐诊断（只诊断，未改代码）

## T0 前置核查

| 项 | 实测 |
|---|---|
| `where ptime` | `C:\Windows\System32\ptime.exe`（`ptime 1.0 for Win32`，Freeware，Jem Berkes） |
| 数据集 | 本机 E 盘无现成同规模树 → 用 `scripts/windows/gen-tree.ps1` 新建 `E:\dev\8c\dataset`：**71 566 个目录 / 116 950 个文件**（用户：71 625 / 116 950；目录少 59 个，文件数相同） |
| 数据集差异 | 本机树为"每目录 1–2 个空文本文件"的合成树（文件平均体积 1 B），用户树为真实目录内容；目录深度本机为 2–3 层 |
| 终端配置 | `E:\dev\8c\t3cfg.toml` = `config/leantty.windows.toml` 复制，仅替换 `[terminal.shell]` 为 `program="cmd.exe"`, `args=["/c","E:/dev/8c/t3run.cmd"]`；网格 `dimensions = { columns = 120, lines = 30 }`；`[renderer] backend` 未改（默认 auto→CPU）；字体默认（Consolas 13） |
| `[perf]` 采集 | `.cmd` 内 `leantty.exe … 2>perf-N.log`（直接句柄重定向）；`LEANTTY_PERF_FRAMES=1` |

计时口径：T1 用 `ptime`（十进制秒）；T2/T3 因输出需落在终端上，改用 `powershell Stopwatch` 包住 `cmd /c "tree …"`（子进程 stdout 直接进终端，不重定向），把 `elapsed_seconds=` 写入标记文件——**与 ptime 口径不同**（Stopwatch 含 1 次 PowerShell 启动后的计时窗，不含 PS 启动本身）。

## T1 基线（`ptime tree <目录> > file`，不渲染）

| run | 原始值 |
|---|---|
| 1 | `Execution time: 17.358 s` |
| 2 | `Execution time: 15.991 s` |
| 3 | `Execution time: 14.242 s` |

中位数 **15.991 s**（文件系统遍历 + 写文件的下限参考）。

## T2 conhost 对照（自启 cmd.exe，`mode con cols=120 lines=30`，tree 输出直接进该控制台）

| run | cmd_pid | 原始值 |
|---|---|---|
| 1 | 15676 | `elapsed_seconds=103.7686192` |
| 2 | 3208 | `elapsed_seconds=113.1129454` |
| 3 | 13784 | `elapsed_seconds=91.1788122` |

中位数 **103.769 s**。三个 cmd 均自行退出（`exit=True`），未按名字结束任何进程。

## T3 leantty（120×30，`cmd /c t3run.cmd`，tree 输出进 ConPTY）

| run | cmd 启动器 pid | wall（含 8 s `timeout`尾巴） | 原始值 |
|---|---|---|---|
| 1 | 19304 | 117.71 s | `elapsed_seconds=106.8448723` |
| 2 | 18124 | 101.10 s | `elapsed_seconds=87.9889501` |
| 3 | 18520 | 44.10 s | `elapsed_seconds=32.8376844` |

中位数 **87.989 s**。每次都有标记文件为"子进程确实跑完"的证据，leantty 在命令结束后随 shell 退出（另设 `timeout /t 8` 保留窗口）。

`[perf]` 侧（`perf-N.log`，24 条统计行/次）：

| run | frames_window 合计 | `chan_depth_max` | `feed_ms_max` | `coalesced` 合计 |
|---|---|---|---|---|
| 1 | 4857 | 1 | 6.3 | 0（`bc` 不可用时用 awk 复核过，见注） |
| 2 | 5750 | 1 | 0.7 | 0 |
| 3 | 3267 | 1 | 0.7 | 0 |

## T4 wezterm

**未测**。三次尝试与失败原因（原始错误）：

1. `--config initial_lines=30` → `Error converting lua value … 'initial_lines' is not a valid Config field. Did you mean one of 'initial_cols', 'initial_rows'?`；
2. msys 环境把 `-- cmd.exe /c …` 的 `/c` 改写成 `C:/`（进程命令行实测 `cmd.exe C:/ E:\dev\8c\t4.cmd`），脚本未执行；
3. 改用 `initial_rows=30` 后启动了自有 `wezterm-gui`（PID 17304），但命令行仍被改写，标记文件未生成。

按红线 **不得结束 wezterm-gui**，故该实例（17304）与另一个既有实例（19372，`start --cwd "C:\vt"`，非本任务启动）均只记录、未结束；T4 数据**不得凭空填写**。

## T5 判定（仅在可测场景内）

| 场景 | 3 次原始值（s） | 中位数（s） | 对 conhost 比值 |
|---|---|---|---|
| 基线（ptime→文件） | 17.358 / 15.991 / 14.242 | 15.991 | 0.154 |
| conhost 120×30 | 103.769 / 113.113 / 91.179 | 103.769 | 1.00 |
| leantty 120×30 | 106.845 / 87.989 / 32.838 | 87.989 | **0.85** |
| wezterm | 未测 | — | — |

**判定：未复现**——leantty 对 conhost 的比值 0.85（< 1.3），即本机 leantty 不比 conhost 慢。
已覆盖条件：窗口网格 120×30、字体默认（Consolas 13）、`backend` 默认（auto→CPU）、数据集 71 566 目录 / 116 950 文件、机器 8 核 / 32 GB（本机）。**未覆盖**：wezterm（三次失败）、用户真实数据集（内容/深度不同）、用户机器（无杀毒但其硬件不同）。

**工程判断（置信度中）**：用户观察到的 41 s vs 93 s 更像是 **wezterm 侧更快**，而不是 leantty 比 conhost 慢；本机 conhost 与 leantty 同量级（甚至 leantty 略快），与"终端渲染是瓶颈、wezterm 的渲染管线更高效"一致。推翻条件：在能测 wezterm 的机器上测得 conhost≈leantty≈100 s 而 wezterm≈40 s，则结论成立；若 wezterm 也≈100 s，则用户数据需重测。

## T6 耗时去向（未触发：比值<1.3，按任务要求不展开定位）

- 现有 `[perf]` 字段（本次实测输出）：`loop_gap_ms`、`feed_ms_max`、`feed_bytes_max`、`writer_queued_max`、
  `frames_window/os/resize/other`、`loops`、`wake_timer/event/pty`、`chan_depth_max`、`coalesced`，以及启动分段
  （`config_ms`、`app_new_ms`、`font_*`、`gpu_*`、`surface_ms` 等）。
- 若后续需要定位，**需要补观测**：读线程单次读取字节数分布、读线程阻塞时长累计、VT `feed` 累计耗时、
  CPU 渲染累计耗时、`present` 累计耗时（当前只有 `*_max` 与帧计数，没有累计和）。

## T7 根因候选（不改代码，供裁定）

| 候选 | 最小修法 | 预估收益（估计） |
|---|---|---|
| 渲染管线效率低于 wezterm（本机未测 wezterm，无法确认） | 无（需先测 wezterm 基线） | 未知 |
| 大数据量下 conhost/leantty 受滚动回看重绘影响 | 限制 scrollback 或增量刷新策略（属行为变更） | 未量化 |
| 数据集差异（空文件/浅层）导致本机数值不可与用户直接对比 | 用户机重测 0.2.1 | — |

## T8 结论状态（一行一条）

- 已验证事实：`ptime` 存在；数据集 71 566 目录 / 116 950 文件；基线 17.358/15.991/14.242 s；conhost 103.769/113.113/91.179 s；leantty 106.845/87.989/32.838 s；leantty/conhost 中位数比值 0.85；wezterm 三次尝试均失败（配置字段名、msys 参数改写）。
- 工程判断：用户"leantty 慢 2.27×"在本机**未复现**；差异更可能来自 wezterm 更快而非 leantty 慢于 conhost（置信度中）。
- 待验证假设：wezterm 在本机的 tree 耗时（未测）；用户机上 0.2.1 的 conhost/leantty 对照（未测）。
- 未改代码；仅新增诊断脚本 `scripts/windows/gen-tree.ps1`（非导出工具）。
