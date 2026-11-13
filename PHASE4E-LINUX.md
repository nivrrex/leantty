# PHASE4E-LINUX：合并 Windows 3E / 启动打点 / 启动优化 / 测试门控放开

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 Windows | 取 `phase3e-win-done-2`（14dd0cf，含另一 tag 且为 win 头）；shared 代码**零改动** |
| t2 启动打点 | 补 CPU 构造完成点 + GPU 五段；未覆盖部分 CPU 7ms / GPU 21ms（≤30ms） |
| t3 启动优化 | 候选 (a) 收益仅 3.1% 放弃；候选 (b) 以"消除重复字体加载"实现，GPU 首帧 **525→379ms（-27.8%）** |
| t4 测试门控 | 8 个文件移除 `#![cfg(unix)]`、去掉绝对路径；Linux 用例数不变（129），Windows target `cargo check` 通过且零告警 |

## 1. t1 合并 Windows（全部 ls-remote 输出）

```
refs/heads/win                         14dd0cfd157ede05f8c23e670d1621534d35e2d3
refs/tags/phase3e-win-done            d5ed02059d38796fa20b996fc28b673e44be4527
refs/tags/phase3e-win-done^{}         fb86374ab7847fe0b4cde5f3336c954170785b32
refs/tags/phase3e-win-done-2          de32464e32a64258aefb0a0d5d36fb0dd2d0fd8c
refs/tags/phase3e-win-done-2^{}       14dd0cfd157ede05f8c23e670d1621534d35e2d3
```

- 两个 tag 解引用后分别是 **fb86374** 与 **14dd0cf**；`git merge-base --is-ancestor` 确认
  fb86374 是 14dd0cf 的祖先，且 14dd0cf 就是 `refs/heads/win` 的头。
- **选择理由**：按任务要求"合并解引用后 commit 最新的 tag"，取 `phase3e-win-done-2`
  → **14dd0cf**（同时满足"含先前 tag"与"最新"）。
- merge-base = `8e7abfe`（4D 交付），`git merge --no-ff` 无冲突 → **890999a**。
- **shared 范围核对**：相对 merge-base 共 55 个文件，全部落在 `vt-verify-win/`
  （含 `logs/phase2b-win/*.err` 与 `run_t3_minimize.ps1`、`run_t4_inverse.ps1` 等探针脚本），
  **shared 代码零改动**，符合"Windows 本轮未改共用代码"。

## 2. t2 启动打点补全

新增（release 可用、无额外分配、命名沿用 `xxx_ms=... since_start=...`）：

| 打点 | 含义 |
|---|---|
| `cpu_renderer_ready_ms` | `CpuRenderer` 构造完成（字体加载之后的收尾） |
| `gpu_instance_ms` | wgpu Instance 创建 + 适配器枚举 |
| `gpu_adapter_ms` | 适配器选定 |
| `gpu_device_ms` | `request_device` 完成 |
| `gpu_atlas_ms` / `gpu_surface_ms` / `gpu_pipeline_ms` | 图集 / surface 创建+configure / 管线+缓冲 |
| `gpu_atlas_pipeline_ms` | 离屏路径的合并点 |

**分段表（release，:99，各 5 次中位）**

CPU 后端：

| 段 | 中位 |
|---|---|
| fontdb 扫描 | 80.4ms |
| 四组字体解析（合计） | ≈75.5ms |
| cpu_renderer_ready（自构造开始） | 158.3ms |
| window | 1.7ms |
| surface | 0.5ms |
| pty_spawn | 4.0ms |
| pre_first_submit | 193ms |
| **first_frame** | **201ms** |

GPU 后端：

| 段 | 中位 |
|---|---|
| gpu_instance | 63.2ms |
| gpu_device | 139.5ms |
| 第二次 fontdb + 四组字体（t3 已消除） | 78.8 + ≈76.1ms |
| gpu_atlas | 0.4ms |
| gpu_surface | 17.8ms |
| gpu_pipeline | 7.1ms |
| renderer_ready | ≈509ms |
| pty_spawn | 6.3ms |
| pre_first_submit | 506ms |
| **first_frame** | **525ms** |

**未覆盖部分**（`first_frame_ms − pre_first_submit_ms`，即首帧渲染本身）中位：
CPU **7ms**、GPU **21ms** —— 均 ≤30ms，无需继续补点。

**t2 的关键发现**：`auto/gpu` 模式下 `build_font_groups` 被调用**两次**（窗口前建 CPU
渲染器估算格子尺寸 + 窗口后建 GPU 渲染器），每次 fontdb ≈79ms + 四组字体 ≈76ms，
重复约 155ms——这是 GPU 首帧中最大的一块可优化项，直接指向 t3 候选 (b)。

## 3. t3 启动优化（各候选单独评估）

### 候选 (a)：fontdb 扫描与窗口创建 / PTY spawn 并行 —— **放弃**

- 实测：fontdb 扫描 80ms；窗口创建 1.7ms + surface 0.5ms + PTY spawn 4.0ms = **6.2ms**。
- 并行收益上限 = min(80, 6.2) ≈ **6.2ms**，占 CPU 首帧 201ms 的 **3.1%**，远低于 15% 保留线。
- 额外障碍：窗口像素尺寸 = 行列 × 格子尺寸，而格子尺寸来自字体度量——**必须先有字体**才能定尺寸。
- 结论：不实现，避免为不足 4% 的收益引入线程与并发风险。

### 候选 (b)：wgpu 初始化与字体扫描并行 —— **以"消除重复"实现并保留**

不做线程并行（需线程所有权、join、panic 处理），而是消除 t2 发现的重复加载：

- `CpuRenderer.groups` / `GpuRenderer.groups` 改为 `Arc<Vec<Vec<Font>>>`（零拷贝共享）；
- 新增 `CpuRenderer::font_groups()` 与 `GpuRenderer::new_surface_with_groups(...)`
  （窗口句柄与尺寸打包成 `SurfaceInit`，避免参数个数超限）；
- app 的 GPU 分支复用窗口前已构建的字体链；`Font` 内含 CJK 字体的大块数据，
  `Arc` 共享避免了 clone 19MB × N 组的成本。
- **无新增线程、无并发失败路径、无新增依赖或配置项**。

**实测（release，:99，各 5 次中位）**

| 后端 | t2 基线 | t3 之后 | 变化 |
|---|---|---|---|
| GPU first_frame | 525ms | **379ms** | **-27.8%** ✅（≥15%） |
| CPU first_frame | 201ms | 194ms | 噪声范围内 |

**验证**：`cargo test --workspace` 129 全过（含 `render_pixels` 绝对像素断言与
`inverse_pixels` 5 例）、`gpu_parity` 8 用例全过、e2e 91/91（case26「增量 vs 全量」
与 case27「滚动后截图」逐像素一致）——渲染输入与输出未变。

## 4. t4 放开纯 CPU 测试的 unix 门控

**改动**：

- 移除文件级 `#![cfg(unix)]` 的 8 个文件：`baseline_alignment`、`glyph_clip`、
  `cell_metrics`、`fallback_chain`、`wide_glyph`、`preedit_ghost`、`background_load`、
  `inverse_pixels`（4C 新增）。核对确认它们只依赖 fontdue/fontdb/CpuRenderer，
  无 `std::os::unix` / `libc` / `nix` 等平台专属 API。
- GPU 相关用例仍各自带 `#[cfg(target_os = "linux")]`；`gpu_parity.rs` 保持文件级
  `#![cfg(target_os = "linux")]`。
- `cell_metrics.rs` 去掉写死的 `/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf`，
  改为按族名（DejaVu Sans Mono → Consolas → Courier New → Liberation Mono）经 fontdb
  查询，全找不到时**显式 SKIP 并打印原因**（不静默通过）。
- `baseline_alignment.rs` 提示文本中的示例绝对路径改为占位符 `<本地 Consolas 字体文件路径>`。
- `glyph_clip.rs` 中仅 Linux 使用的 `settings()` 与相关 import 加 `#[cfg(target_os = "linux")]`
  （Windows 下 GPU 用例被排除，否则会有 dead_code/unused 告警）。

**用例数前后对比（Linux）**：均为 **129 passed / 0 failed、2 ignored**，逐文件：

| 测试目标 | 用例数 |
|---|---|
| app lib | 44 |
| app keyboard | 5 |
| app reader | 4 |
| config | 13 |
| pty_unix | 1 |
| pty_windows | 0（Linux 上整文件 cfg 排除） |
| render lib | 12 |
| background_load | 2 |
| baseline_alignment | 2 |
| cell_metrics | 1 |
| fallback_chain | 3 |
| glyph_clip | 2 |
| gpu_parity | 8 |
| incremental | 3 |
| inverse_pixels | 5 |
| preedit_ghost | 2 |
| bench_frame / font_load_time | 各 0（ignored 1） |

**Windows 目标**：本机已安装 `x86_64-pc-windows-gnu`，
`cargo check --target x86_64-pc-windows-gnu --tests -p render` **通过且零告警**。

## 5. 验收

- `cargo test --workspace`：**129 passed / 0 failed**（2 ignored）
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**91/91 PASS**，`GPU_SKIP=0`，GPU 轮 **6/6**
- Windows 依赖树 `wgpu`/`pollster` 命中：**0**（每个 tag 前复核）
- Windows target `cargo check --tests -p render`：通过、零告警

## 6. 未解决问题

1. **GPU 首帧仍有 ≈200ms 的 wgpu 初始化**（已验证事实：instance 63 + device 139）：本机
   NVIDIA 470 驱动上 `request_device` 就占 139ms，属驱动侧成本。若要进一步压缩，只能
   把 GPU 初始化整体前移到窗口创建之前（需 `compatible_surface=None` 先建实例再补 surface，
   wgpu 允许），但那会让 `auto` 模式在无 GPU 时机也付出探测成本；未实施，登记为待评估。
2. **候选 (a) 未实现但并非无价值**（工程判断，置信度中）：若将来把窗口创建也改成
   "先按默认尺寸建窗、字体到位后 `request_inner_size` 修正"，则字体扫描可与窗口并行，
   收益上限会提高到 ~80ms（去掉尺寸依赖后）。当前不做的理由是打字/尺寸抖动的 UX 风险。
   **推翻条件**：若首帧要求下探到 150ms 以内，需重新评估该方案。
3. **Windows 侧未运行全套测试**（已验证事实）：本机只有 `cargo check`，未在真实 Windows
   运行 129 个测试；t4 的目标（Windows 可编译且用例不再被排除）达成，但运行结果未核实。
4. **`cargo check` 与 `cargo test` 的差异**：Windows 侧的字体查询行为（族名 "Consolas"
   是否存在、fontdb 在 Windows 上的 generic 解析）**未核实**，需 3F 复验。

## 7. Windows 3F 交接

1. **运行测试**：`cargo test -p render` 在 Windows 上应不再整片跳过——`baseline_alignment`、
   `glyph_clip`、`cell_metrics`、`fallback_chain`、`wide_glyph`、`preedit_ghost`、
   `background_load`、`inverse_pixels` 现在都会执行（GPU 用例仍按 Linux cfg 排除）。
   若字体族名在 Windows 上不匹配，会打印 `SKIP:`（`cell_metrics` 的候选列表已含 Consolas
   与 Courier New）。
2. **复验 GPU 首帧收益**：3E 报告 Windows release 首帧 764ms（其中 359ms 在
   `create_renderer` 内、字体日志之后）。t2 已在该区间补齐打点（`gpu_instance_ms` /
   `gpu_device_ms` 等），t3 消除了重复字体加载——Windows 上请用新打点确认 359ms 的构成，
   并核对首帧是否随之下降。
3. **Windows 启动仍可能是 CPU 后端**：`auto` 在 Windows 一律回落 CPU（wgpu 不在依赖树），
   所以 t3 的 GPU 收益在 Windows 上不适用；CPU 路径的重复加载本就不存在，
   3E 的 764ms 需用 t2 新打点重新归因。
4. **诊断开关沿用**：`APP_PERF_FRAMES`、`APP_DUMP_SNAPSHOT`、`APP_FRAME_HASH`、
   `APP_TEST_PASTE` 均 debug 专有（`APP_PERF_FRAMES` release 可用）。

## 8. 手动清单（≤3）

1. office-pc 上确认启动观感（t3 后 CPU 首帧 ≈194ms、GPU ≈379ms），重点看 GPU 模式
   首个画面是否明显提前。
2. Windows 3E/3F：用新打点复核 764ms 首帧的构成（`gpu_instance_ms`/`gpu_device_ms`
   在本机是不可用的，需 Windows 侧数据）。
3. 若将来把首帧目标压到 150ms 以内，请确认是否接受"先建默认尺寸窗口、字体到位后修正
   尺寸"带来的轻微尺寸跳变（这是候选 (a) 提收益的前提）。
