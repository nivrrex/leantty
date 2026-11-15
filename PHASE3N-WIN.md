# PHASE3N-WIN：合并 4O、今晚测试包 kit v1（不含 GPU）与本机试跑

- 日期：2026-11-15（按系统时间）
- 范围：合并 `phase4o-done`；制作 `scripts/windows/tonight/` 下的 kit v1（不含 GPU 构建），在本机（无 GPU 的构建机）用 `-Quick` 与默认参数各完整试跑一次，修脚本缺陷；`pack-kit.ps1` 出包。**未改 `crates/` 下任何代码，未加依赖，二进制与 zip 均不入库。**
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4o-done`（`git ls-remote`） | `f04f777a72d66ba77ff8206941070dcd011b9353`，只有一行（**轻量 tag，无 `^{}`**），`git cat-file -t` = `commit` |
| 合并 | `git merge --no-ff f04f777`，**0 冲突**，合并 commit `370f752` |
| Windows 专属文件整体 sha256（合并前后） | 101 个含 "win" 的文件（排除 `vt-verify-win/`）整体 **`d2f152ee200c2e92aeb9b5e747a512cf8915d6166fc30d2bc92257038b1a3a76` 一致** |
| debug / release 构建 | 均通过；**release 有 1 条 warning**（见下，upstream 4O 引入） |
| `cargo clippy -D warnings`（dev / release） | **两配置均失败**：`error: unused import: eperf`（`crates/render/src/lib.rs:63`，4O t4 新增的 `pub(crate) use eperf;`） |
| `cargo test --workspace` | **183 passed / 0 failed / 2 ignored**，rc=0 |
| `frame_bytes_identical`（4O 起带字体指纹） | **SKIP**：实测指纹 `DejaVuSansMono.ttf:340712:53dd:8x16`，期望 `DejaVuSansMono.ttf:343140:c69b:8x16`；本机当前 `frame_hash=507485fb7945fb8f`（与 GOLDEN 相同）但仍按指纹判 SKIP，故该黄金断言在 Windows 上不再生效 |
| `cargo tree` 中 `wgpu`、`pollster` | 命中 **0** |

- **未修**上述 clippy/警告：属 `crates/` 共用代码，按规则只登记（请求 1）。因此本轮 `clippy -D warnings = 0` 这一项**未达成**，如实记录。

## 2. t2 kit 结构（`scripts/windows/tonight/`）

| 文件 | 作用 |
|---|---|
| `run-tonight.ps1` | 唯一入口；`-SoakMinutes`（默认 5）、`-SkipSoak`、`-OutDir`（默认 `.\results`）、`-Quick` |
| `lib.ps1` | DPI（PerMonitorV2）、显示器与 DPI 读取、按 PID 找窗口、聚焦（AttachThreadInput + 点击）、输入注入（keybd_event/VkKeyScanW，含注入生效验证 `Confirm-Typing`）、`CopyFromScreen` 截图与像素比较（LockBits）、直接句柄重定向启动（`launch-*.cmd` 内 `cd /d` + `set APP_PERF_FRAMES=1` + `2>log`）、内存/句柄采样、`[perf]` 解析、8.3 短路径 |
| `configs/cpu.toml`、`gpu.toml`（`backend="gpu"`）、`gpu-auto.toml`（`backend="auto"`） | 字体/颜色/键位沿用 `examples/windows.toml`，无绝对路径；`shell` 统一为 `cmd.exe`（键入命令的确定性需要，README 已说明） |
| `README-TONIGHT.md` | ≤1 页：前置条件、命令、预计耗时、3 条手动项 |
| `pack-kit.ps1` | 出包（见 t5） |

规则落实：脚本内所有路径都由 `$PSScriptRoot` 派生；不联网、不写注册表/系统设置/防火墙/全局环境变量；只结束自己启动的 `app.exe`（PID 记录在句柄里）；从不触碰 OpenConsole/conhost/wezterm/pwsh。

## 3. t3 阶段实现（P0–P4）

- **P0 环境**：OS/CPU/RAM、`Win32_VideoController`（Name/DriverVersion/AdapterRAM/VideoProcessor/PNPDeviceID/当前分辨率）、先 `SetProcessDpiAwarenessContext(PerMonitorV2)` 再 `EnumDisplayMonitors` + `GetDpiForMonitor` 读每屏 DPI 与缩放、RDP 判定（`SESSIONNAME` + `query session`）、只读枚举 `HKLM:\SOFTWARE\Khronos\Vulkan\Drivers`（本机 0 项）、`dxdiag /whql:off /t`（90 s 超时，超时只结束自己启动的 dxdiag）、`app.exe` sha256 与版本。
- **P1 CPU 基线**：5 次冷启动 first_frame；注入 `sample`（`sample.cmd` → `sample.ps1`，写 `sample-done.txt` 作为注入生效证据）后截图 `S_cpu.png`（ASCII/汉字/全角/7 色/反色行）；30 s 空闲内存；两次吞吐（30 万行、120×30 与 200×60 各自独立会话）；20 次缩放；最小化/恢复与"自建窗口遮挡再移走"的前后截图对比。
- **P2 GPU**：三种变体（不设环境变量 / `WGPU_BACKEND=dx12` / `=vulkan`）。**N/A 判定以 `[perf] gpu_init_ms` 是否出现为准**——只有真正走通 GPU 路径才会打印；`backend="gpu"` 在无 GPU 构建上打印"在当前平台不可用，回退 cpu"并被判 N/A（首次实现用 `backend=gpu` 字样判定，`auto` 变体会漏判，已修）。RDP 会话下整段 SKIP 并在 summary 顶部加醒目告警。
- **P3 浸泡**：每后端按 `-SoakMinutes` 跑，30 s 采样 WS/Private/句柄/线程；负载 = 周期输出 + 两次缩放 + 每 3 轮一次 1 MB 真实路径粘贴（.NET 写剪贴板 + 右键）；结束检查 stderr 无 panic/backtrace，并记录关窗是否正常退出。
- **P4 汇总**：`summary.md`（顶部环境/RDP/GPU/阶段表，其后关键数字）与 `summary.json`，并把整个 run 目录压成 `results\tonight-<时间戳>.zip`（压缩失败重试 5 次，仍失败才抛）。

每个阶段独立 `try/catch` + 时限（`$script:Deadline`，阶段内循环自检）；某阶段失败不阻断后续，最终一定出 zip。

## 4. t4 本机试跑（解压到含空格与中文的路径，从 `C:\Windows` 启动）

路径：`…\vt-verify-win\kit-trial 试跑\`（12 个文件）。

| 运行 | 参数 | 总耗时 | P0 | P1 | P2 | P3 | P4 | zip |
|---|---|---|---|---|---|---|---|---|
| 试跑 | `-Quick` | **5.7 min** | PASS 26.7s | PASS 231.4s | **N/A** 12.3s | PASS 67.1s（1 min 浸泡） | PASS 1.8s | 生成 |
| 正式 | 默认（浸泡 5 min） | **10.1 min** | PASS 26.3s | PASS 231.7s | **N/A** 12.3s | PASS 331.0s | PASS 1.9s | 生成（83 575 B / 24 项） |

默认参数实测关键数字（release，`APP_PERF_FRAMES=1`，直接句柄重定向）：

- `first_frame_ms [cpu]`：n=5 中位 **419 ms**、最小 **338 ms**（与 3M 的 419/395 同量级）。
- 空闲内存（30 s）：ws **269.2 MB** / priv 272.5 MB / 句柄 251 / 线程 10。
- 吞吐 120×30：**63.1 s**（19.9 MB / 30 万行），render med 4 ms、present med 2 ms、feed **374 794 B/s**、coalesced 99。
- 吞吐 200×60：**66.8 s**，render med 11 ms、present med 6 ms、feed **280 308 B/s**、coalesced 92。
- 最小化→恢复对比：**diff 0/0/0**（max_diff 0、diff_ratio 0、结构性差异 0）。遮挡→移走对比：**diff 0**。
- 20 次缩放后与缩放前的对比：max_diff 252、diff_ratio **3.3%**、12 列结构性差异——与"输出增多后出现滚动条（12 列宽）"一致；**只记录，未判失败，未归因**。
- 浸泡（5 min，10 个采样）：ws 270.4 → **274.9 MB**，panic=False，关窗正常退出=True。
- 环境：Win10 LTSC 19044、Xeon 8378C 4C/8T、16 GB、显卡 `ELINK DISPLAY WDDM DRIVER`（虚拟适配器，VRAM 0）、单屏 1920×1080@96dpi/1.0、Vulkan ICD 0 项、非 RDP、`dxdiag` 退出码 0。

**试跑中发现并修复的脚本缺陷（8 项）**：

1. `launch-*.cmd` 用 `-Encoding ASCII` 写入 → 含中文的 kit 路径被写成 `??`，app 起不来。改 `-Encoding Default`（ANSI/GBK）。
2. 窗口内 shell 的 cwd 是用户配置目录，不是 kit 目录 → 键入的相对命令落空。在 `launch-*.cmd` 里加 `cd /d "<WorkDir>"`，并额外用 8.3 短路径（ASCII、无空格）在会话内 `cd /d`。
3. `sample.ps1` 里写的是绝对路径且经 ASCII 编码 → 路径含 `??` 抛异常。改为只写相对文件名 `sample-done.txt`。
4. 吞吐等待时限用 `[int]($Deadline - (Get-Date)).TotalSeconds * 1000` → 优先级错误，算出 3 s。改为先取剩余秒数再 `Min/Max`。
5. 吞吐完成标记写到 `..\tp120.done` 而等待路径是 run 目录 → 永远等不到。统一为 kit 根目录下的短名。
6. `[perf] frames n=50 …` 没有 `key=` 形式，`Get-PerfLine` 匹配不到 → 表里显示"no frames line"。为 frames 单写匹配器。
7. GPU 判定用 `backend=gpu` 字样 → `auto` 变体（Windows 上静默回落 CPU）被误判为"GPU 可用"，进而多跑一段假 GPU 浸泡。改为只认 `gpu_init_ms`。
8. 缩放/最小化/遮挡的截图基线用的是早期 `S_cpu.png` → 输出变化导致的滚动条差异被算进"结构性差异"（12 列）。改为同一动作的**前后**两张对比，最小化/遮挡因此回到 0。

另外：早期 `-Quick` 试跑曾因第 1/2 项残留一个 `app.exe`（11952，kit 的 `gpu-auto` 配置）与其子进程 `OpenConsole.exe`（5644）；已按 PID 关闭该 app，`OpenConsole` 随父进程自行退出（未对 OpenConsole 做任何操作），进程集合回到 `328,9376` / conhost 25。

## 5. t5 kit 包

`scripts/windows/tonight/pack-kit.ps1`（路径全参数化；ConPTY 两文件与 NOTICE.md 沿用固定 hash 校验；失败非零退出且不留半成品）：

| 项 | 值 |
|---|---|
| kit 目录 | `vt-verify-win/tonight-kit/`（gitignore），12 个文件，**9 167 934 B** |
| zip | `vt-verify-win/tonight-kit-v1.zip`（gitignore），**3 460 316 B** |
| zip sha256 | **`45a93931cf187f4377e2852f9992c92f99b7a05f268f8be8c11639fd7e866ae4`** |
| 组成 | `app.exe`（7 866 405 B，sha256 `7e79336e…`）、`conpty.dll`（119 608 B，`e971856f…`）、`OpenConsole.exe`（1 089 336 B，`b17e48b0…`）、`LICENSE-ConPTY.txt`、`NOTICE.md`（`e7fbaade…`）、`run-tonight.ps1`、`lib.ps1`、`configs/{cpu,gpu,gpu-auto}.toml`、`README-TONIGHT.md`、`MANIFEST.txt` |

（首次打包的中间产物 `ef3c12ea…` 已被最终版覆盖，报告中只保留最终值。）

## 6. 未解决问题

1. `clippy -D warnings` 在两种配置下均因 upstream 4O 的 `pub(crate) use eperf;` 失败——本轮门槛未达成（不改 `crates/`）。
2. `frame_bytes_identical` 在 Windows 上恒为 SKIP（黄金指纹绑定 Linux 的 DejaVu 文件尺寸），黄金断言实际不再执行。
3. P2 在本机构建上恒为 N/A，GPU 阶段代码路径（截图比较、适配器信息、GPU 浸泡）**本轮未被真实执行**，只做了语法与流程审查；目标机若为 GPU 构建，这是首次真实运行。
4. 缩放前后 3.3% 像素差（12 列 == 滚动条宽度）未归因。
5. 键入注入依赖"先点窗口再打字"，本机 3 次试验均成功，但目标机若有 IME/焦点策略差异，可能出现 `tp-ready*.done` 缺失而 P1 失败——届时 summary 的 note 会直接给出该原因。

## 7. 对共享代码 / 观测的请求

1. **（CI，P1）** `crates/render/src/lib.rs:63` 的 `pub(crate) use eperf;` 未被使用，`clippy -D warnings` 与 release 构建均报 `unused import`；删掉该行或在模块内显式使用即可。状态=已验证事实（本机两配置均失败）。
2. **（测试，P2）** `frame_bytes_identical` 的 `GOLDEN_FINGERPRINT` 绑定单一字体文件（`DejaVuSansMono.ttf:343140:c69b`）；Windows 侧 DejaVu 为 340712 B，指纹不符即 SKIP，等于该平台失去黄金校验。建议按"指纹集合"或按平台记录期望值。
3. **（沿用 3M）** `terminal.shell.args` 在 Windows 被替换为 `/K "chcp 65001"`；kit 因此只能用 `cmd.exe` 并靠键入命令驱动，若将来支持 args，kit 可改为配置驱动、去掉键入依赖。

## 8. 手动清单（≤3）

1. 把 `vt-verify-win/tonight-kit-v1.zip`（sha256 `45a93931…`）拷到目标机，解压到本地任意目录，用**物理登录**运行 `powershell -NoProfile -ExecutionPolicy Bypass -File run-tonight.ps1`，把生成的 `results\tonight-*.zip` 回传。
2. 若目标机是 GPU 构建，确认 summary 里 P2 不是 N/A，并勾选 README 的三条手动项（拖到副屏/改缩放、GPU 观感与功耗、中文输入法与 Ctrl+Shift+C）。
3. 若 P1 报 `tp-ready*.done missing`，说明键入注入未生效：先手动点一下窗口再重跑，并把该现象回报。

## 9. 推送与 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `4b79b14779d47df3ad36b3e5eb6f383108a501ba` |
| `refs/tags/phase3n-win-done`（tag 对象） | `1254d2576b592e79d892f351cbc49aafbc87fd85` |
| `refs/tags/phase3n-win-done^{}`（解引用 commit） | `4b79b14779d47df3ad36b3e5eb6f383108a501ba` |

- `git push origin win`：`558e061..4b79b14  win -> win`；tag 新建后 push（`* [new tag]`）。未 force、未移动任何已有 tag。
- 提交内容：`PHASE3N-WIN.md`、`docs/WIN-NOTES.md`（5 条 3N 事实 + 请求 1/2）、`scripts/windows/tonight/*`（5 个文件）、`vt-verify-win/logs/3n/*-summary.md`（两次试跑的汇总副本）、`.gitignore`（kit 输出目录、zip、试跑目录、3n 的 json）。**zip 与所有二进制均未入库**（提交前检查 .exe/.dll/.zip/.png 命中 0）。
