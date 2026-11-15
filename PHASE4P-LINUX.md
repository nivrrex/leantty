# PHASE4P-LINUX：Windows 上启用 wgpu GPU 后端（Vulkan + DX12）

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3m-win-done` | 已合并 `f475a84`（tag `phase4p-t1`），Windows 侧零共用代码改动 |
| t2 Windows GPU 后端 | 依赖/门控/语义/选择/日志全部落地；dx12 在 GNU 目标下**编译并链接成功**（tag `phase4p-t2`） |
| t3 文档 | README 新增「Windows GPU 后端（实验性）」（tag `phase4p-done`） |

## 1. t1 合并证据（ls-remote 实测）

```
f04f777a72d66ba77ff8206941070dcd011b9353	refs/heads/main（合并前）
558e0613b0e28690308a834e61d87df1df3cfca2	refs/heads/win
bea283fd7d715c18058e28024ef453eb235274a9	refs/tags/phase3m-win-done
f475a8467c8913d94e28c2a2d3fcda6c0f808b13	refs/tags/phase3m-win-done^{}
```

- tag 为**附注标签**（有 `^{}` 行），解引用 = `f475a84`；`refs/heads/win` = `558e061`，
  比它多一个"record push/tag hashes and commit contents"提交（仅改 `PHASE3M-WIN.md`，+8/−1）。
- Windows 侧自有提交（`f475a84` / `c4b0a48`）**未触及 `crates/`**（逐提交 `git diff-tree` 核对为 0）；
  该区间里的 `crates/` 改动全部来自 main 的 4N 提交。
- 合并后相对 main 的共用/受控范围：`.gitignore`、`PHASE3L-WIN.md`（上一轮漏下的 3L 补记提交带入）、
  `PHASE3M-WIN.md`、`docs/WIN-NOTES.md`；`crates/` 零差异。

## 2. t2 改动清单

### 2.1 依赖（`crates/render/Cargo.toml`）

```toml
[target.'cfg(any(target_os = "linux", target_os = "windows"))'.dependencies]
pollster = "0.4"
raw-window-handle = "0.6"

[target.'cfg(target_os = "linux")'.dependencies]
wgpu = { version = "=30.0.2", default-features = false, features = ["vulkan", "wgsl"] }

[target.'cfg(target_os = "windows")'.dependencies]
wgpu = { version = "=30.0.2", default-features = false, features = ["vulkan", "dx12", "wgsl"] }
```

- **dx12 在 `x86_64-pc-windows-gnu` 下可用**：`cargo build --release --target x86_64-pc-windows-gnu -p app`
  成功（4m26s），**未退到 vulkan-only**。
- 版本精确锁定 `=30.0.2`（不升级）。
- 规模：Windows 目标 normal 依赖 **288 个 crate**；`app.exe` **18,088,760 字节**。
  改前 Windows 目标不含 wgpu，体积增量**未核实**（未在同一环境重新构建基线）。

### 2.2 平台门控

- `crates/render/src/gpu/mod.rs` 文件级 `#![cfg(...)]`、`render/src/lib.rs` 的 `gpu` 模块导出与
  `GpuRenderer` 相关 `cfg`、`app/src/lib.rs` 的 `RendererBackend::Gpu` 及其 19 处分支、
  以及 6 个 GPU 测试文件（`gpu_parity`、`glyph_clip`、`baseline_alignment`、
  `cursor_viewport_pixels`、`inverse_pixels`、`inverse_default_color`）全部由
  `cfg(target_os = "linux")` 放宽为 `cfg(any(target_os = "linux", target_os = "windows"))`。
- **gpu 模块内无 X11/Wayland 专属代码**：surface 仅经 `wgpu::SurfaceTargetUnsafe::RawHandle`
  创建（Win32 句柄由 winit 提供），因此**不需要**平台隔离模块。
- Linux 默认字体常量仍为 `cfg(target_os = "linux")`（否则与 Windows 的 `Consolas` 定义冲突）。

### 2.3 后端选择语义

| 配置 | Linux | Windows |
|---|---|---|
| `backend = "cpu"` | CPU | CPU |
| `backend = "auto"` | 先试 GPU，失败回落 CPU（不变） | **直接 CPU**（不尝试 GPU） |
| `backend = "gpu"` | 尝试 GPU；失败返回错误（非零退出） | **同上**（删除原来的"回退 CPU 并警告"分支） |

后端决定仍发生在**首个 present 之前**：`create_renderer` 在窗口创建后、softbuffer surface
创建前决定；Windows 上 `auto` 走 CPU 分支时不会创建任何 wgpu surface，不存在"两套 surface 并存"。

### 2.4 适配器选择与可观测

- `wgpu::Backends::from_env()`（标准环境变量 `WGPU_BACKEND`）优先；未设置时
  Linux=`VULKAN`、Windows=`VULKAN | DX12`。
- 先排除 `DeviceType::Cpu`（Windows 的 WARP / Basic Render Driver 属此类）；
  若设置了 `WGPU_ADAPTER_NAME`，按**名字子串（不区分大小写）**匹配，未匹配时返回错误并列出可用名；
  否则按 Discrete > Integrated > 其他 的类型优先级、同级保持枚举顺序。
- `APP_PERF_FRAMES=1` 时输出：每个适配器一行
  `[perf] gpu_adapters name=… backend=… type=… driver=… driver_info=… vendor=… device=…`、
  被选中适配器（含 backend/type/driver）、`gpu_instance_ms … backends=…`、
  `gpu_surface_ms … fmt=… present=… alpha=… size=…`。
- **Linux 选择结果与改前一致**（实测日志）：

```
[perf] gpu_instance_ms=76.8 since_start=225.0 backends=Backends(VULKAN) count=2
[perf] gpu_adapters name='NVIDIA GeForce GT 710' backend=Vulkan type=DiscreteGpu driver='NVIDIA' driver_info='470.256.02' vendor=0x10de device=0x128b
[perf] gpu_adapters name='llvmpipe (LLVM 15.0.6, 256 bits)' backend=Vulkan type=Cpu driver='llvmpipe' ...
[perf] gpu_adapter_ms=76.9 since_start=225.1 name='NVIDIA GeForce GT 710' backend=Vulkan type=DiscreteGpu driver='NVIDIA'
[perf] gpu_surface_ms=22.2 since_start=362.0 fmt=Bgra8Unorm present=Fifo alpha=Opaque size=1080x504
```

### 2.5 其余路径

沿用 Linux 实现：整帧重绘、surface `Lost` 重建 / `Outdated` reconfigure、device lost 重建一次、
resize 与缩放因子变化重新配置 surface、不回读帧缓冲。

### 2.6 门禁脚本

`scripts/gates.sh` 第 6 项由"Windows 目标 `wgpu` 命中为 0"改为"**Windows 目标 wgpu 版本 ==
30.0.2**"（实测输出 `Windows 目标 wgpu: wgpu v30.0.2`）；`tag_checked.sh` 未改。

## 3. 验证证据

| 项 | 结果 |
|---|---|
| `cargo check --target x86_64-pc-windows-gnu --workspace --tests` | 通过，**0 error / 0 warning** |
| `cargo clippy --target x86_64-pc-windows-gnu --workspace --all-targets -- -D warnings` | 通过 |
| `cargo build --release --target x86_64-pc-windows-gnu -p app` | **成功**（4m26s），产物 18,088,760 B |
| Windows 目标依赖 | 288 crate；`wgpu v30.0.2` 且特性含 `dx12` + `vulkan` + `wgsl` |
| Linux 全量测试 | **174 passed / 0 failed**（render 61；含 gpu_parity 8 例、glyph_clip、baseline_alignment、cursor_viewport_pixels、inverse_*、palette_*，全部阈值/逐字节不变） |
| Linux e2e | **106/106**（GPU 轮 10/10：smoke、截图阈值、反色、括号粘贴、OSC4/104、auto 回落、无 ICD exit=1） |
| 门禁 | `scripts/gates.sh` PASS（含新的 wgpu 版本检查） |

### 3.1 期间修正的一处用例判据

`case47` 的"被拒绝的粘贴不改变视口"原先用"拇指比例不变"判定。阶段 4P 回归时 gpu 轮出现
`0.958→0.889` 的假失败：拒绝期间后台仍在回显此前**已被接受**的粘贴内容，回滚缓冲变长会让
同一绝对位置的相对比例自然下降。判据改为"**未回底（<0.98）且不跳向底部（≤ 前值+0.02）**"，
语义更贴合规格，且仍能证伪"回底"。

## 4. Windows 3O 需要验证的清单

1. **构建**：`cargo build --release`（MSVC 与 GNU 两种工具链）是否都通过；本机只在
   `x86_64-pc-windows-gnu` 上验证过。产物体积与基线差（本机未核实）。
2. **无 GPU 机器上 `backend="gpu"`**：应以非零码退出并打印清晰错误（Linux 端已用"无 ICD"
   场景覆盖 `gpu-round-gpu-noicd-exit`）；Windows 上的实际错误文本/退出码待验。
3. **`backend="auto"`**：应仍为 CPU（不尝试 GPU），启动日志（开 `APP_PERF_FRAMES`）中不应出现
   `gpu_adapters`。
4. **环境变量行为**：`WGPU_BACKEND=dx12` / `WGPU_BACKEND=vulkan` 能否分别选中；
   `WGPU_ADAPTER_NAME` 子串匹配与未匹配时的错误（应列出可用适配器名）。
5. **present 行为**：DX12 与 Vulkan 两个后端下窗口内容、resize、最小化恢复是否正常
   （Linux 的 surface Lost/Outdated 路径无法在 Windows 上等价复现）。
6. **WARP/Basic Render Driver**：若机器只有软件适配器，应报"仅软件实现可用"而不是选中它。

## 5. 未解决问题

1. **真实 Windows GPU 上完全未验证**（present、DX12/Vulkan 差异、驱动兼容）：本轮只做到
   "能在 Windows 目标编译链接 + Linux 行为不变"。
2. **Windows `auto` 仍是 CPU**：待 3O 数据再决定是否让 auto 尝试 GPU（推翻条件见 DECISIONS）。
3. **Windows release 体积增量未核实**：改前基线未在同一环境重建。

## 6. 手动清单（≤3）

1. Windows 机器上 `cargo build --release` 通过后，用 `backend="gpu"` + `APP_PERF_FRAMES=1`
   跑一次，把 `gpu_adapters` / 选中行 / `gpu_surface_ms` 贴回（用于确认 DX12 是否被选中）。
2. 在同一机器上分别试 `WGPU_BACKEND=dx12` 与 `WGPU_BACKEND=vulkan`，对比 present 是否都正常。
3. 在**无 GPU** 的 Windows（或禁用显卡的 VM）上试 `backend="gpu"`，确认非零退出与错误信息。
