# PHASE3Q-WIN：合并 4Q、WGPU_BACKEND 行为、WARP（DX12 软件适配器）全程验证、kit v4

- 日期：2026-11-16（按系统时间）
- 范围：合并 `phase4q-done`；在无真实 GPU 的机器上用 **WARP（Microsoft Basic Render Driver）** 把 GPU 代码路径完整跑一遍；核对 `WGPU_BACKEND` 在 Windows 上的行为；把 kit 的 P2 升级为"真实 GPU 优先、无适配器时用 WARP 兜底"。**未改 `crates/`，未加依赖，二进制与 zip 均不入库。**
- perf 日志一律 `APP_PERF_FRAMES=1` + 直接句柄重定向；环境变量一律 `set "K=V"`。
- 重要前提：**WARP 是软件渲染，不暴露真实驱动的问题；本文所有 GPU 数据只说明"逻辑路径可用"，不能外推到真实 GPU。**

## 0. 进程处理

- 未结束任何 `OpenConsole.exe` / `conhost.exe` / `wezterm-gui.exe` / `pwsh.exe`。
- 只结束本任务自己启动、命令行核对过的 `app.exe`（WARP 试验、paste 试验、kit 试跑）。
- 全程 `OpenConsole` PID 集合恒为 `328, 9376`；结束时 `app_left=0`；`conhost` 30→32（早前阶段遗留，按规则只记录）。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4q-done`（`git ls-remote`） | `1e10fbc0293bb96f54718a183e855b33635db02d`，只有一行（**轻量 tag，无 `^{}`**） |
| 合并 | `git merge --no-ff 1e10fbc`，**0 冲突**，合并 commit `ebd7d40` |
| Windows 专属文件整体 sha256 | `7bfb4c21587bb9bb22bf5c17b3365593e9288f6fa53b9a3480716b28a424ff49`，**合并前后一致** |
| debug / release | 均通过；**两者零告警**（release 增量 20 s，app.exe 18 097 959 B） |
| clippy（dev / release，`-D warnings`） | **0 / 0** |
| `cargo test --workspace` | **203 passed / 0 failed / 2 ignored**（404 s） |
| GPU 用例（默认无开关） | **SKIP**，打印 `SKIP: 无可用真实 GPU 适配器（未找到真实 GPU 适配器（仅软件实现可用））`；覆盖 `gpu_parity`(8)、`inverse_pixels`、`baseline_alignment`、`glyph_clip`、`cursor_viewport_pixels` 的 `*_gpu` 用例 |
| GPU 用例（`APP_GPU_ALLOW_SOFTWARE=1`） | **`gpu_parity` 8/8 通过**（199.9 s，走 WARP）——即 4Q 的软件适配器开关在测试里**生效**，只是默认关闭 |
| `wgpu` | **精确 30.0.2**（`cargo tree --target x86_64-pc-windows-gnu`） |

## 2. t2 WGPU_BACKEND 在 Windows 上的行为（4Q 自有解析）

`backend="gpu"`，每种取值一个独立 wrapper（`set "WGPU_BACKEND=..."`），`APP_PERF_FRAMES=1`，直接句柄重定向：

| 设置值 | `backends_env` | `backends_effective` | 适配器数 | 退出码 | 耗时 | 错误原文（首行） |
|---|---|---|---|---|---|---|
| `dx12` | `'dx12'` | `Backends(DX12)` | 1（Basic Render Driver, Cpu） | 1 | 1.66 s | `未找到真实 GPU 适配器（仅软件实现可用）` |
| `DX12` | `'DX12'` | `Backends(DX12)` | 1（同上） | 1 | 0.80 s | 同上 |
| `dx12 `（尾随空格） | `'dx12 '` | `Backends(DX12)` | 1（同上） | 1 | 0.86 s | 同上 |
| `vulkan` | `'vulkan'` | `Backends(VULKAN)` | **0** | 1 | 0.83 s | `适配器枚举为空：请求的后端（WGPU_BACKEND='vulkan' → Backends(VULKAN)）在本平台/本构建不可用或没有对应驱动` |
| `dx12,vulkan` | `'dx12,vulkan'` | `Backends(VULKAN \| DX12)` | 1（DX12 WARP） | 1 | 0.92 s | `未找到真实 GPU 适配器（仅软件实现可用）` |
| `vk` | `'vk'` | `Backends(VULKAN)` | 0 | 1 | 0.81 s | 适配器枚举为空（同上措辞） |
| `dx` | `''`（解析失败前未打印 instance 行） | — | — | 1 | 0.77 s | `WGPU_BACKEND='dx' 含无法识别的后端记号 ["dx"]；支持：vulkan\|vk, dx12\|d3d12, metal\|mtl, gl\|gles\|opengl, webgpu` |
| `""`（空） | `'<unset>'` | 默认 `VULKAN \| DX12` | 1（DX12 WARP） | 1 | 0.84 s | `未找到真实 GPU 适配器（仅软件实现可用）` |
| `not-a-backend` | — | — | — | 1 | 0.81 s | `WGPU_BACKEND='not-a-backend' 含无法识别的后端记号 ["not-a-backend"]；…` |

结论（已验证事实）：4Q 的严格解析在 Windows 上按预期工作——大小写无关、尾随空格被裁掉、`dx12,vulkan` 组合正确、无法识别的记号**直接报错并回显原始值**、空值视作未设置走默认集合。DX12 请求下枚举到 **Microsoft Basic Render Driver（type=Cpu）** 但因"非真实适配器"被过滤；Vulkan 请求下**枚举为空**（本机无 Vulkan 驱动）。

## 3. t3 WARP 全程验证

启动参数：`backend="gpu"`、`APP_GPU_ALLOW_SOFTWARE=1`、`WGPU_BACKEND=dx12`、`WGPU_ADAPTER_NAME=Basic`。

### 3.1 a) 启动（10 次，直接句柄）

| 指标 | 值 |
|---|---|
| `first_frame_ms` | 中位 **477 ms** / 最小 **395 ms**（n=10） |
| 用户可见警告 | `正在使用软件 GPU 适配器：Microsoft Basic Render Driver，仅用于诊断（APP_GPU_ALLOW_SOFTWARE=1）`（**1 行，不受 perf 开关门控**） |
| `gpu_instance_ms` | `backends_env='dx12' backends_effective=Backends(DX12) count=1` |
| `gpu_adapter_ms` | `name='Microsoft Basic Render Driver' backend=Dx12 type=Cpu driver='10.0.19041.4355'` **`software=true`** |
| `gpu_surface_ms` | `fmt=Bgra8Unorm present=Fifo alpha=Opaque size=960x480` |
| `gpu_init_ms` | **107.6 ms** |

### 3.2 b) 内容与像素（CPU 同内容 vs WARP）

同一份注入样例（ASCII/汉字/全角/7 色/反色行，`sample-done.txt` 为注入生效证据），`shot.ps1`（PerMonitorV2 子进程）截图：

| 指标 | 值 |
|---|---|
| 最大通道差 | **252** |
| 差异像素占比 | **2.22%** |
| >32 的像素占比 | **1.46%** |
| 结构性缺行 / 缺列 | **0 / 12** |

- 12 列结构性差异与滚动条宽度一致（两图内容长度不同导致滚动条出现/隐藏），与 3N/3P 的 CPU-vs-CPU 缩放对比同源；整屏黑/空白**未出现**（`Get-BlackRatio` 远低于 0.9）。
- kit 的 P2 warp 变体在**同尺寸**下测得 `max_diff=252 diff_ratio=0.83% gt32_ratio=0.03% blank_rows=0 blank_cols=0`，文字区域基本一致。

### 3.3 c) 空闲 20 秒（4Q 修复的复验）

- 20 秒内产生 **4 条 `loop_gap_ms` 报告**（5 s 节拍），每条 `frames_window=0 frames_os=0 frames_resize=0 frames_other=0` —— **空闲渲染帧数 = 0**（4Q 在 Linux 上把 205→0 的修复，在 Windows/DX12 软件路径上**同样成立**）。
- 同一行还带 `gpu acquire: success=93 suboptimal=0 outdated=0 lost=0 other=0`（累计计数，非窗口计数；无 suboptimal/lost）。

### 3.4 d) 缩放 / 最小化 / 遮挡

| 动作 | 结果 |
|---|---|
| 20 次缩放后 vs 缩放前 | max_diff 252、ratio **0.28%**、结构性缺行/列 **0/0** |
| 最小化→恢复 | **max_diff 0** |
| 自建窗口覆盖→移走 | **max_diff 0** |
| panic / surface 错误 | 无（日志仅有正常 surface 行） |

### 3.5 e) 吞吐（30 万行，120×30）

| 后端 | 完成时间 | render 中位 | present 中位 | feed | coalesced |
|---|---|---|---|---|---|
| **WARP (DX12)** | **128.4 s** | 9 ms | 0 ms | 186 201 B/s | 338 |
| CPU（同口径，3P/kit） | 49–62 s | 3–4 ms | 1 ms | 394 000–493 000 B/s | ~100 |

WARP 比 CPU 慢约 2–2.6 倍——**这是软件光栅化的预期结果，不能代表任何真实 GPU 的性能**。

### 3.6 f) 粘贴与 loop_gap

| 项目 | 结果 |
|---|---|
| 1 MB 粘贴（真实路径，右键触发） | **1 048 576 字节精确**，8.8 s |
| 5 MB 粘贴 | **5 242 880 字节精确**，40.8 s |
| 5 MB 粘贴中 Ctrl-C | 送达 **65 531 字节** |
| `loop_gap` 最大（ping -t + 5 MB 粘贴） | **39 ms**（< 100 ms） |

### 3.7 g) 浸泡 5 分钟（每 30 s 采样）

| 时间 | ws | priv | 句柄 | 线程 |
|---|---|---|---|---|
| 30 s | 316.6 MB | 541.3 MB | 331 | 16 |
| 128 s | 316.6 MB | 541.3 MB | 331 | 14 |
| 226 s | 316.7 MB | 541.3 MB | 341 | 15 |
| 325 s | 316.7 MB | 541.2 MB | 333 | 13 |

- 无泄漏趋势（ws 波动 0.2 MB、priv 波动 0.1 MB；句柄 331–341、线程 12–16 属采样抖动）；无 panic；关窗正常退出。

### 3.8 h) 关窗

- 关窗耗时 **236 ms**；stderr 无 panic/backtrace；仅 3 条非 perf 行：`gpu_surface_ms`、`surface_ms`、`copy primary: clipboard error: primary selection unavailable: Windows 无 PRIMARY`（后者为已知平台差异）。

### 3.9 i) 失败项

无失败项；未发现需要改共用代码的缺陷。

## 4. t4 WARP 与 CPU 对照（仅记录）

| 指标 | CPU | WARP (DX12) | 说明 |
|---|---|---|---|
| `first_frame_ms`（10 次中位/最小） | 414 / 320（3P 实测） | **477 / 395** | WARP 初始化多约 60 ms（`gpu_init_ms` 107.6、`gpu_adapter_ms` 34） |
| 空闲内存 ws / priv | 269.5 / 270.5 MB | **316.6 / 541.3 MB** | WARP 的 D3D12 设备与堆占用显著更高（priv +270 MB） |
| 吞吐 30 万行 | 49–62 s | **128.4 s** | 软件光栅化，慢 2–2.6 倍 |
| render 中位 / present 中位 | 3–4 ms / 1 ms | 9 ms / 0 ms | present 为 0 说明 Fifo 下无阻塞 |
| 最小化/遮挡恢复 | diff 0 | **diff 0** | 一致的逻辑行为 |
| 空闲帧数 | 0 | **0** | 4Q 修复在两个路径上都成立 |

**不外推**：WARP 不模拟厂商驱动的 present 队列、多 GPU、显示器热插拔与真实缩放路径，以上数字只代表"逻辑路径可用"。

## 5. t5 kit v4

### 5.1 改动

1. **P2 新增 "warp" 兜底变体**：仅当前三个真实变体都因"无可用真实适配器"失败时才运行，设置 `APP_GPU_ALLOW_SOFTWARE=1` + `WGPU_ADAPTER_NAME=Basic` + `WGPU_BACKEND=dx12`；**真实变体绝不设置该变量**。
2. **状态区分**：真实 GPU 跑通 → `PASS`；只有 WARP 跑通 → **`PASS (WARP only)`** 并在 summary note 中写明"no real GPU adapter; WARP ran instead — covers surface/present/resize/loop logic only, NOT real driver behaviour"；都不可用 → `N/A`。summary.json 增加 `gpu_mode`（`real` / `warp-only` / `none`）。
3. P2 每个变体新增：注入样例截图与 `S_cpu.png` 的逐像素对比、**空闲 20 秒帧数统计**（`frames_window/os/resize/other` 求和）、30 秒空闲内存、30 万行吞吐。
4. 归档改为**子进程** `zip-run.ps1`（流式 `ZipArchive` → `.partial` → 改名；>50 MB 只存末尾 5 MB；逐文件容错），并在 README 写明 P2 与 WARP 的含义。

### 5.2 包

| 项 | 值 |
|---|---|
| 目录 | `vt-verify-win/tonight-kit-v4/`（gitignore），**14 文件 / 19 422 288 B** |
| zip | `vt-verify-win/tonight-kit-v4.zip`（gitignore），**6 632 625 B** |
| zip sha256 | **`713b4844b843492dd3ee771d7c34917f99bb654ad2d33c75b884418af8b9c467`** |
| 新增文件 | `zip-run.ps1`（2 806 B，sha256 `9050a0df…`）；`shot.ps1` 4 477 B（`be423811…`）；`run-tonight.ps1` 与 `lib.ps1` 同步更新 |

### 5.3 试跑（解压到含空格与中文的路径，从 `C:\Windows` 启动）

| 运行 | 路径 | 总耗时 | P0 | P1 | P2 | P3 | P4 | zip |
|---|---|---|---|---|---|---|---|---|
| `-Quick` | `…\kit v4 中文 路径\`（前一轮 `kit-trial v4` 同包） | **5.8 min** | PASS 30.3s | PASS 210.6s | **PASS (WARP only)** 168.6s | PASS 135.3s | PASS 1.2s | 110 081 B |
| 默认（5 min 浸泡） | `…\kit v4 中文 路径\` | **17.9 min** | PASS 29.1s | PASS 212.6s | **PASS (WARP only)** 165.8s | PASS 665.4s | PASS 1.4s | 119 577 B |

- P2 warp 实测（kit 内）：`requested DX12, effective Backends(DX12)`、`software=true`、`fmt=Bgra8Unorm present=Fifo alpha=Opaque 960x480`、`vs S_cpu max_diff=252 diff_ratio=0.83% gt32_ratio=0.03% blank 0/0`、**`idle 20 s -> 0 frame(s)`**、`idle memory ws 314.6 / priv 540.3 MB`、`throughput 120x30 59.5 s`。
- 两次运行 `OpenConsole` PID 集合前后都是 `328,9376`，`app_left=0`，未结束任何非自己启动的进程。

### 5.4 本轮修掉的 kit 缺陷（7 项）

1. P2 只有三个真实变体，无 GPU 机器上永远 N/A → 增加 warp 兜底变体与三态结论。
2. `Confirm-Typing` 只接受 session 对象，P2 传窗口句柄时报 `找不到属性 Hwnd` → 改为两者都接受。
3. zip 在主进程执行会**卡死**（该进程已加载 WinForms/Drawing，混用 `System.IO.Compression`）→ 拆成子进程 `zip-run.ps1`。
4. `& powershell.exe @args` 使用 PowerShell 自动变量 `$args`，splat 后子进程变成**交互式等待输入**（挂起）→ 改名 `$zipArgs`。
5. `summary.json` 序列化整个 `$script:Env`（含 perf 行、采样数组）会**卡死** → 只序列化原语与小对象（明细留在 environment.json / soak-*.json）。
6. summary.json 读取 `$r.note`，而 stage 记录里的字段名是 `error` → 兼容读取。
7. `@(...) | Where-Object` 返回单元素时不是数组，`.Count` 在 StrictMode 下报错 → 双重 `@()`。

## 6. 未解决问题

1. **WARP 不等于真实 GPU**：present 队列、多显示器/缩放、驱动层错误路径均未覆盖；真实 GPU 数据仍缺（本轮不再可得）。
2. `diff_ratio 0.83–2.22%` 的具体构成（滚动条 + 光标）未逐项归因；`max_diff 252` 的来源（个别像素）未定位。
3. WARP 空闲 priv 541 MB（CPU 270 MB）的构成未拆分。
4. 真实 Vulkan 路径在本机无驱动（枚举 0），只能靠目标机历史数据（2026-11-15，AMD 核显，最大差 1 LSB）。
5. `conhost` 数量 30→32 为早前阶段遗留，未处理（按规则只记录）。

## 7. 对共享代码的请求

1. **（观测，P3）** 建议在 `gpu_surface_ms` 之后补一条 `gpu_backend_api`（adapter.get_info().backend 已有，但 surface 侧的 present 队列/帧延迟配置没有单独打点），便于把 WARP 与真实 GPU 的 present 行为区分开。
2. **（文档）** 4P/4Q 的 `WGPU_BACKEND` 文档应注明：Windows 上 `vulkan` 在没有 ICD 的机器上会直接报"适配器枚举为空"，这是预期失败而非缺陷。
3. 无功能性缺陷需要修（本轮未发现需要改共用代码的问题）。

## 8. 手动清单（≤3）

1. 有真实 GPU 的机器上运行 kit v4，确认 P2 状态是 `PASS`（而不是 `PASS (WARP only)`），且 summary 里 `gpu_mode=real`、`software=false`。
2. 若只有 WARP，请把 `PASS (WARP only)` 的结果与真实 GPU 的数据分开比较，不要混用性能数字。
3. 回传 `results\tonight-*.zip`，附 README 的三条手动项观察（切显示器/缩放、观感与功耗、中文输入法）。

## 9. 推送与 tag（ls-remote 实际输出）

见文末补记。
