# PHASE3P-WIN：kit 在真实 GPU 机器上暴露的缺陷修复、环境变量传递验证、P4 OOM 排查

- 日期：2026-11-15（按系统时间）
- 范围：修复目标机运行 tonight-kit-v2 暴露的 kit 自身缺陷（P3 时限、P4 OOM、DPI 感知、P2 断言与补全），验证 `WGPU_BACKEND` 的真实传递路径。**未改 `crates/`，未加依赖，二进制与 zip 均不入库。**
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- perf 日志一律 `APP_PERF_FRAMES=1` + 直接句柄重定向；性能与内存只用 release。

## 0. 进程处理

- 未结束任何 `OpenConsole.exe` / `conhost.exe` / `wezterm-gui.exe` / `pwsh.exe`。
- 只结束本任务自己启动、命令行核对过的 `app.exe`（kit 试跑、环境变量探针、截图测试）。
- 全程 `OpenConsole` PID 集合恒为 `328, 9376`；`app_left=0`；`conhost` 数量 30（与 3O 结束时一致，非本轮产生）。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4q-done` | **origin 上不存在任何 `phase4q*` tag** → **未合并 4Q**，本轮只在 `win` 分支工作 |
| release 构建 | 通过，**零告警**（增量 2 s） |
| app.exe | 18 087 652 B（与 3O 相同，4P 合并后未变） |
| Windows 专属文件 | 本轮未改 `scripts/windows/**` 之外的 Windows 文件；kit 改动集中在 `scripts/windows/tonight/` |

## 2. t2 环境变量传递（WGPU_BACKEND）

### 2.1 kit 生成的实际命令行（逐字，`-Quick` 试跑捕获）

```
@echo off
cd /d "C:\dev\tty\vt-verify-win\3p-kit"
set "APP_PERF_FRAMES=1"
set "WGPU_BACKEND=dx12"
"C:\dev\tty\vt-verify-win\3p-kit\app.exe" --config "...\configs\cpu.toml" 2>"...\logs\app.log"
```

（v2 起就是 PowerShell 环境块 + cmd 包装，**从来没有** `set NAME=value && app` 这种形态；本轮把它也一并做了对照。）

### 2.2 四种传法的对照（backend="gpu"，本机无真实 GPU，进程都会 exit=1）

| 传法 | 命令行原文（关键行） | 日志 `backends=` |
|---|---|---|
| A `set WGPU_BACKEND=dx12 && app`（带空格，假设中的 bug 形态） | `set WGPU_BACKEND=dx12 && echo set-ok` | `Backends(VULKAN \| DX12)` |
| B `set WGPU_BACKEND=dx12&& app`（无空格） | `set WGPU_BACKEND=dx12&& echo set-ok` | `Backends(VULKAN \| DX12)` |
| C `set "WGPU_BACKEND=vulkan"`（规范写法） | `set "WGPU_BACKEND=vulkan"` | `Backends(VULKAN \| DX12)` |
| D kit 的 `Start-AppDirect`（PowerShell 环境块 → cmd → app） | `set "WGPU_BACKEND=vulkan"` | `Backends(VULKAN \| DX12)` |
| E PowerShell `$env:WGPU_BACKEND='dx12'` + `Start-Process` | 无包装 | 无可读 `gpu_instance` 行（该方式用被禁止的重定向，仅作对照，日志仅 264 B） |

结论（已验证事实）：**四种写法结果完全一致**，`WGPU_BACKEND` 对后端选择**没有任何影响**——因此目标机看到的 `backends=Backends(VULKAN | DX12)` **不是尾随空格造成的 kit 缺陷**。

### 2.3 根因（源码 + 探针双证据）

- 探针 `vt-verify-win/probe_wgpuenv`（同版本 `wgpu = 30.0.2`）：设置 `WGPU_BACKEND=dx12` / `vulkan` / `vulkan,dx12` / `DX12` / `dx12 `（尾随空格）后，`Backends::from_env()` **始终返回 `None`**，而 `std::env::var("WGPU_BACKEND")` 能正确读到值。
- 源码：`wgpu-types-30.0.2/src/backend.rs:156` 的 `from_env()` 调 `crate::env::var("WGPU_BACKEND")`；`wgpu-types-30.0.2/src/env.rs` 里该函数在 **未启用 `std` feature 时恒返回 `None`**。
- 本项目的依赖声明：`wgpu = { version = "=30.0.2", default-features = false, features = ["vulkan","dx12","wgsl"] }`；`cargo tree -e features` 中 **没有 `wgpu-types feature "std"`**（wgpu 的 `default` 特性里才含 `std`）。
- 结论（已验证事实 + 高置信度推断）：**`default-features = false` 关掉了 `std`，使 `WGPU_BACKEND` 在编译期就被忽略**。修复点在 `crates/render/Cargo.toml`（共用代码），本轮未改，入请求栏。

### 2.4 kit 侧修复

1. 环境变量一律用 **规范无空格写法** `set "K=V"`（`lib.ps1` 的 `Start-AppDirect -EnvVars`）。
2. P2 新增断言：记录"请求的后端"与"日志中的实际 `backends=`"，并写入 `summary`：
   - 无可用 GPU 路径时 → **N/A**（并把不一致作为附注写明）；
   - 有 GPU 路径运行时，若实际后端与请求不符 → 该变体判 **FAIL** 并写明原因。
   本机实测输出：`dx12: requested DX12, got VULKAN | DX12 (WGPU_BACKEND is not honored)`。

## 3. t3 P3 阶段时限

- 公式：`limit = 后端数 × SoakMinutes × 60 + 180`（`-Quick` 用 1 分钟后端时间，同一公式）。后端数 = 1（仅 CPU）或 2（CPU + GPU 可用时）。
- 实测：`-Quick` → `soak 1 backend(s) x 1 min (limit 240s)`；默认 5 分钟 → 480 s；目标机（2 后端 × 5 分钟）→ **780 s**，而 3O 版的固定 540 s 确实不够（目标机 P3 报 `stage time limit exceeded` 的直接原因）。
- README-TONIGHT.md 已写明公式与预计耗时。

## 4. t4 P4 的 OutOfMemoryException

### 4.1 候选逐个证伪/证实

| 候选 | 方法 | 结论 |
|---|---|---|
| A. zip 把自身或持续增长的文件包含进来 | 读 3O 代码 + 路径核对 | **证伪**：zip 写在 `OutDir`（RunDir 的父目录），`Compress-Archive -Path "$RunDir\*"` 不可能包含它 |
| B. P3 超时后遗留 app 仍持有日志并写入 | 用独占句柄模拟（`run-locked`） | **证实**（是失败但不是 OOM）：`Compress-Archive` 与流式写入都抛 `文件正由另一进程使用`；这正是"P4 前必须先收掉自己启动的 app"的理由 |
| C. PowerShell 5.1 `Compress-Archive` 的内存行为 | 154.5 MB 单文件实测 | **证实（与 OOM 一致）**：`Compress-Archive -Level Optimal` 耗时 4.2 s、**进程峰值 WS 355 MB**（基线 108 MB，约 1.7 倍输入）；输入再大就会线性吃内存。目标机 1029 s 后 OOM 与该行为吻合（本机 run 目录只有约 1 MB，**未能在本机复现同规模 OOM**：属"机制成立、现象未复现"，置信度中） |
| D. 对大文件整体 `ReadAllText/ReadAllBytes` | 代码审查（3O 的 P4） | **证伪**：P4 从未整读文件 |

### 4.2 修复（kit v3）

1. **流式 zip**：`System.IO.Compression.ZipArchive` 逐文件 `CreateEntry` + 64 KB 缓冲写入，先写 `<name>.zip.partial`，成功后 `Move-Item` 原子改名；输出 zip 位于 RunDir 之外并显式排除自身。
2. **大文件规则**：单文件 > 50 MB 只压缩**末尾 5 MB**，并在 `summary.md` 的 notes 中标注。实测（`-SelfTest`）：60 MB 文件 → zip 内条目长度 **5 242 880 B**，`truncated=[logs/big.log]`。
3. **单文件容错**：每个文件 `try/catch`，读不到就跳过并记 note，不让一个坏文件毁掉整包。
4. **P4 前置清理**：新增 `Stop-AllKitApps`——只结束**本 kit 记录过的 PID**且命令行仍指向 kit 目录的 `app.exe`，随后才压缩。
5. **最小 zip 兜底**：主路径失败时再产出一个"跳过 >50 MB 文件"的最小包。实测 `-ForceZipFailure`：`ZIP-FAILED forced zip failure` → `ZIP-MINIMAL … 84 473 bytes`，解压后 **24 个文件、`summary.md` 存在**。
6. P4 自身时限 240 s → **300 s**。

### 4.3 验证

| 场景 | 结果 |
|---|---|
| 正常（默认参数，v3 包） | `ZIP … 89 054 bytes`，Expand-Archive 解开 |
| 强制主路径失败（`-ForceZipFailure`） | 产出最小 zip 84 473 B，含 summary.md 与日志，可解压 |
| 50 MB 规则（`-SelfTest`） | 60 MB → 条目 5 242 880 B，note 标注 |
| 独占文件（模拟遗留 app） | 两种策略都报共享冲突 → 已由"P4 前 Stop-AllKitApps"消除该前置条件 |

## 5. t5 DPI 感知

- 本机实际缩放：**96 dpi / scale 1.0（100%）**，非 150%（按任务要求记录实际值）。
- 失效原因（已验证事实 + 中置信度推断）：
  - 事实：**全新进程的第一条语句**调用 `SetProcessDpiAwarenessContext(PerMonitorV2)` → 返回 **True**；
  - 事实：同一进程**加载 `System.Windows.Forms` 之后**再调用 → 返回 **False**（本机实测）；
  - 推断：目标机上 kit 的 `lib.ps1` 在 dot-source 期间就通过 `Add-Type`/GDI+ 相关装配把进程定型，P0 再调用自然失败 → `dpi_aware=false`（未逐条验证是哪一个装配触发）。
- 修复：新增 **`shot.ps1`**，在**独立子进程的第一条语句**设置 PerMonitorV2，然后按需执行 `info`（显示器/DPI 枚举）、`size`（客户区像素）、`shot`（`CopyFromScreen` 截图）；`lib.ps1` 的 `Save-Shot` 与 P0 的显示器采集全部改为调用它。
- 验证：`-Quick` 试跑 `summary.json` → `dpi_aware=true`、`dpi_aware_in_process=true`、`displays=[{monitor:1920x1080@(0,0), dpiX:96, scale:1}]`；截图尺寸与客户区一致（`size` 与 PNG 尺寸相同）。
- 150% 下的实际效果**未验证**（本机 100%）：预期修复后目标机的截图将不再被系统虚拟化为 2/3 尺寸，需在目标机复核（手动项 2）。

## 6. t6 P2 补全

P2 现在与 P1 用同一套 helper，每个变体（default / dx12 / vulkan）在 GPU 路径可用时执行：

1. **同一份样例内容注入**（`sample.cmd` → `sample.ps1`，写 `sample-done.txt` 作为注入生效证据；ASCII/汉字/全角/7 色/反色行），随后截图 `S_gpu_<backend>.png` 并与 `S_cpu.png` 比较：max channel diff、>32 占比、结构性缺行/列；
2. **30 秒空闲内存 + 空闲帧计数**（统计 `[perf] frames` 报告条数增量，用来发现"GPU 空闲仍持续渲染"）；
3. **一次 300 000 行吞吐**（120×30），记录总时长与 `[perf] frames` 的 render/present/feed。

本机无真实 GPU 适配器，三个变体全部 **N/A**（原因逐条写明），GPU 成功路径**本轮仍未真实执行**；同一套代码路径在 CPU 变体（P1，`-Quick` 与默认各一次）上实际跑通（注入、截图对比、空闲采样、吞吐）。属"代码审查 + CPU 路径实测"级别的验证。

## 7. kit v3 包

| 项 | 值 |
|---|---|
| 目录 | `vt-verify-win/tonight-kit-v3/`（gitignore），**13 文件 / 19 408 084 B** |
| zip | `vt-verify-win/tonight-kit-v3.zip`（gitignore），**6 624 848 B** |
| zip sha256 | **`3ed32ddbc804d899f26ad84347c5918793247287a33fcfdccea7deb27a2c8f30`** |
| 新增/变更 | 新增 `shot.ps1`（4 477 B）；`run-tonight.ps1` 44 971 B（P2 重写、P3 公式、P4 流式 zip/兜底、`-SelfTest`/`-ForceZipFailure`）；`lib.ps1`（`-EnvVars` 规范写法、`Invoke-ShotHelper`、`Stop-AllKitApps`）；`README-TONIGHT.md`（时限公式 + 已知限制） |
| app.exe | 18 087 652 B，sha256 `addb0723…`（与 v2 相同） |

试跑（本机，解压到 `…\kit-trial v3\`，从 `C:\Windows` 启动）：

| 运行 | 总耗时 | P0 | P1 | P2 | P3 | P4 | zip |
|---|---|---|---|---|---|---|---|
| 默认（5 min 浸泡） | **10.2 min** | PASS 32.2s | PASS 230.9s | **N/A** 16.1s | PASS 332.1s | PASS 1.5s | 89 054 B |
| `-Quick`（前一轮） | **5.6 min** | PASS 32.1s | PASS 223.4s | **N/A** 16.9s | PASS 66.9s | PASS 0.6s | 84 113 B |

默认运行数字：`first_frame_ms` n=5 中位 **497 ms** / 最小 386；空闲 ws 271.1 MB；吞吐 120×30 **62 s**（feed 394 695 B/s、render 4 ms）、200×60 **60.8 s**（feed 358 152 B/s、render 11 ms）；最小化/遮挡 diff 0；浸泡 268.9→272.5 MB 正常退出。

### 本轮修掉的 kit 缺陷（9 项）

1. P3 固定 540 s 时限 → 按后端数与时长计算（目标机超时的直接原因）。
2. `Compress-Archive` 内存随输入线性增长 → 流式 `ZipArchive` + `.partial` 原子改名。
3. 大文件无上限 → >50 MB 只存末尾 5 MB 并在 summary 标注。
4. 一个不可读文件毁掉整包 → 逐文件 `try/catch` 跳过并记 note。
5. 主路径失败无输出 → 最小 zip 兜底。
6. P3 超时可能遗留自己的 app 持日志 → P4 前 `Stop-AllKitApps`（仅本 kit 的 PID）。
7. DPI 感知在进程内设置无效 → `shot.ps1` 子进程首语句设置并承担全部截图/枚举。
8. `Invoke-ShotHelper` 把参数加了引号导致路径带引号（截图失败）→ 改为逐参数传递。
9. `Stop-AllKitApps` 返回空数组时 `.Count` 在 StrictMode 下报错；`.Replace('\\','/')` 的转义被写坏（空串）→ 均已修正。

## 8. 未解决问题

1. `WGPU_BACKEND` 在本构建无效（wgpu `std` 特性被 `default-features=false` 关闭）——**共用代码问题**，未修。
2. P4 的 OOM 在目标机（1029 s）**未在本机复现**；机制（`Compress-Archive` 峰值 ≈ 1.7× 输入）已实测，属"机制成立、现象未复现"。
3. GPU 成功路径（内容对比、空闲帧计数、GPU 吞吐）在本机仍无法执行；仅 CPU 路径实测 + 代码审查。
4. DPI 修复在 150% 缩放下的实际效果未验证（本机 100%）。
5. 20 次缩放后 3.3%（12 列 = 滚动条宽）像素差仍未归因（沿用 3N 记录）。

## 9. 对共享代码的请求

1. **（构建配置，P1）** `crates/render/Cargo.toml` 的 `wgpu` 用 `default-features = false` 且未启 `std`，导致 `WGPU_BACKEND` 被编译期丢弃（4P 文档宣称可用）。建议加 `"std"` 特性（或改用带 `std` 的 default 组合），否则 `backends_env/backends_effective` 类观测与后端选择在 Windows 上永远无效。
2. **（观测，P2）** 4Q 若新增 `backends_env` / `backends_effective`，请直接打印**带引号的原始字符串**（尾随空格/大小写一目了然），本轮的 P2 断言即可直接消费。
3. **（沿用）** `frame_bytes_identical` 的指纹绑定单一 DejaVu 尺寸，Windows 恒 SKIP。

## 10. 手动清单（≤3）

1. 目标机（150% 缩放、真实 GPU）解压 kit v3 后跑默认参数：确认 P2 的 `backends=` 实际值与请求一致（若仍为 `VULKAN | DX12`，说明请求 1 未修，属于已知产品限制），并检查摘要里的 `dpi_aware=true`。
2. 目视核对截图尺寸：150% 缩放下 `S_cpu.png` / `S_gpu_*.png` 应等于窗口客户区的物理像素（不再是系统虚拟化后的 2/3）。
3. 回传 `results\tonight-*.zip`，附 README 的三条手动项观察。

## 11. 推送与 tag（ls-remote 实际输出）

见文末补记。
