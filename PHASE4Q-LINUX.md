# PHASE4Q-LINUX：合并 3O / WGPU_BACKEND 严格解析 / GPU 空闲持续渲染修复 / 软件适配器开关

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3o-win-done` | 已合并 `b3c07a4`（tag `phase4q-t1`） |
| t2 `WGPU_BACKEND` 解析 | wgpu 会**静默丢弃**未知后端；已改严格解析 + 双字段日志（tag `phase4q-t2`） |
| t3 GPU 空闲持续渲染 | **Linux 真实 GPU 复现**（205 帧/5s）；根因是"静止仍被定时唤醒且 GPU 真 present"；已修，0 帧（tag `phase4q-t3`） |
| t4 软件适配器开关 | `APP_GPU_ALLOW_SOFTWARE=1` 仅 `backend="gpu"` 生效；lavapipe 全用例通过（tag `phase4q-t4`） |

## 1. t1 合并证据（ls-remote 实测）

```
d2fc1da9abdc8590622781dafa5f2c199975dd68	refs/heads/main（合并前）
8ea6d299f1f0601f0f59e05c1394e43e80a6f02d	refs/heads/win
d2e5b456c4dc24cb8611acb8cadf9fa6398eca5e	refs/tags/phase3o-win-done
b3c07a41ecb707a29341b04a369bbb53f3c24a4e	refs/tags/phase3o-win-done^{}
```

- tag 为**附注标签**（有 `^{}`），解引用 = `b3c07a4`；`refs/heads/win` = `8ea6d29`，比它多一个
  仅改 `PHASE3O-WIN.md` 的补记提交（+8/−1）。按任务要求合并解引用后的 `b3c07a4`
  （其中也包含更早的 `phase3n-win-done` 内容）。
- Windows 侧自有提交中只有 `fd52b80` 触及 `crates/`，且仅两个 Windows 专属文件
  （`crates/pty/src/windows.rs`、`crates/pty/tests/pty_windows.rs` — 成对部署警告）。
- 合并后相对 main 的共用/受控范围：`.gitignore`、`PHASE3M-WIN.md`、`PHASE3N-WIN.md`、
  `PHASE3O-WIN.md`、`docs/WIN-NOTES.md` 与上述两个 Windows 专属文件。

## 2. t2 WGPU_BACKEND 的处理（表）

| 环节 | 4P 之前（wgpu 自带） | 本次（本仓库严格解析） |
|---|---|---|
| 读取方式 | `Backends::from_env()` → `from_comma_list(&env)` | `std::env::var` + `parse_backends_spec` |
| 空白 | 每项 `trim()`（内部已做） | 每项 `trim()`（显式，覆盖首尾/制表/换行） |
| 大小写 | `to_lowercase()` | `to_ascii_lowercase()`（等价） |
| 逗号列表 | 支持 | 支持（空项忽略、重复项合并） |
| 无法识别的记号 | **静默丢弃**；全不匹配时仅 `log::warn!`（本仓库无日志器 → 用户不可见），可能得到空集合 | **返回 Err 并携带原始值与出错记号** |
| 空值/仅分隔符 | 得到空集合 | Err |
| 可观测 | 无 | `gpu_instance_ms … backends_env='<原始值>' backends_effective=<解析结果>` |

单测 6 条（`crates/render/src/gpu/mod.rs::backends_spec_tests`）覆盖：
`dx12`、`DX12`、`"dx12 "`（尾随空格）、`" vulkan"`、`"vulkan,dx12"`、`vk`、`dx`、空/仅分隔符、
`乱码`、以及 `metal|mtl|gl|gles|opengl|webgpu` 全支持项。

**Linux 实测**：

| 输入 | 结果 |
|---|---|
| `vulkan` | `backends_env='vulkan' backends_effective=Backends(VULKAN)`，2 个适配器 |
| `vulkan `（尾随空格） | 同上（完全一致） |
| `dx12` | 适配器 0 个 → `适配器枚举为空：请求的后端（WGPU_BACKEND='dx12' → Backends(DX12)）在本平台/本构建不可用或没有对应驱动`，非零退出 |
| `dx` / `乱码` | `WGPU_BACKEND='…' 含无法识别的后端记号 [...]；支持：vulkan|vk, dx12|d3d12, metal|mtl, gl|gles|opengl, webgpu` |

> 对 2026-11-15 那次"变量没生效"的判断：**不是尾随空格**（wgpu 内部本就 trim），更可能是变量
> 未进入进程环境，或值不被识别后被静默丢弃。现在两种情况都会留下证据（日志或明确报错）。

## 3. t3 GPU 空闲持续渲染

### 3.1 取证表（哪些路径会在空闲时触发重绘）

| 位置 | 空闲时是否触发 | 说明 |
|---|---|---|
| 帧节流 `!due` 分支的 `set_control_flow(WaitUntil(last_render+interval))` | **是（根因）** | 每次渲染后都排一次定时唤醒；静止画面也会被叫醒 |
| `about_to_wait` 的 `due` 判定 | **是（根因）** | `due` 时无条件渲染；CPU 空 damage 提前返回，GPU 真 present |
| surface `Suboptimal` | 否（按 Success 处理） | 修复前后计数均为 0（实测 suboptimal=0） |
| surface `Outdated` / `Lost` | 否 | 计数 0；仅异常时重配/重建 |
| `Resized` / `ScaleFactorChanged` | 事件驱动，非空闲 | 空闲窗口内 `frames_resize=0/1` |
| OS `RedrawRequested` | 事件驱动 | 空闲窗口内 `frames_os=0` |
| 拖选自动滚动 `request_redraw` | 否（`mouse_down && auto_scroll != 0`） | 空闲时 mouse_down=0 |
| 粘贴排空 `request_redraw` | 否（`pending_paste` 非空才走） | 空闲时队列为 0 |

**修复前实测（Linux 真实 GPU，NVIDIA GT 710）**：空闲 5 秒窗口 `frames_window=205`、`frames_os=0`、
`frames_resize=1`、`frames_other=204`、`success=205`、`feed_bytes=0` —— 与 Windows 3O 的
"17 秒 1050 帧、每帧约 5ms、feed_bytes=0"同源。

### 3.2 修复

在**渲染前**加一条空闲判定：

```rust
if is_gpu && !urgent_before && !snap.full_redraw && snap.is_clean()
    && geom_now == self.last_geom_drawn {
    self.update_ime_cursor_area();
    return;   // 不渲染、不 present
}
```

- 滚动条几何提前计算并记录 `last_geom_drawn`，保证"滚动条变化仍会重绘"；
- 紧急触发（输入/粘贴/OS 重绘/缩放）来自 `urgent_before`，仍然立即绘制；
- **CPU 后端路径未改**（CPU 本来就会在 damage 为空时提前返回，只是白算一帧）。

### 3.3 新增观测

- `GpuRenderer::AcquireStats { success, suboptimal, outdated, lost, other }`（`Suboptimal` 与
  `Success` 共用绘制路径但分开计数）；
- 每 5 秒的 `loop_gap_ms` 行追加 `frames_window / frames_os / frames_resize / frames_other`，
  **只统计真正绘制的帧**（空闲跳过不计入），并附 GPU acquire 计数。

### 3.4 门禁与证伪（`case49`，:99、release、真实 GPU）

| 版本 | 输出停止后的空闲窗口 | 缩放后窗口 |
|---|---|---|
| 修复后（scale 1.0） | **0** | **0** |
| 修复后（`WINIT_X11_SCALE_FACTOR=1.5`） | **0** | **0** |
| 回退修复（临时关掉空闲判定） | **220 / 206** | **302 / 335** |

判据 ≤3 帧；修复后 2/2 PASS，回退 0/2 → **证伪成立**。
另用 lavapipe（t4 的开关 + `VK_ICD_FILENAMES=lvp_icd.x86_64.json`）重复同一用例：
空闲窗口 `14 0`、缩放后 `0 0`（首个窗口 14 帧为止住时的收尾）→ 同样满足判据。

## 4. t4 软件适配器开关

- `APP_GPU_ALLOW_SOFTWARE=1`：仅当配置为 `backend="gpu"` 时把 `DeviceType::Cpu` 适配器纳入候选；
  `backend="auto"` **永远忽略**（实测：auto + 开关 + 只有 lavapipe ICD → `[renderer] gpu 不可用，
  回落 cpu：…未找到真实 GPU 适配器（仅软件实现可用）`，`backend=cpu`）。
- 选中软件适配器时 stderr 默认输出一行用户可见警告
  （`正在使用软件 GPU 适配器：llvmpipe …，仅用于诊断（APP_GPU_ALLOW_SOFTWARE=1）`），
  诊断日志 `gpu_adapter_ms … software=true`。
- 排序不变（Discrete > Integrated > 其他），软件适配器只在没有真实适配器时被选中，
  除非用 `WGPU_ADAPTER_NAME` 指定它。

### 4.1 lavapipe 与真实 GPU 的对照（同一套阈值，未放宽）

| 用例 | lavapipe | 真实 GPU（NVIDIA GT 710） |
|---|---|---|
| gpu_parity | **8 passed** | 8 passed |
| glyph_clip | 2 passed | 2 passed |
| baseline_alignment | 2 passed | 2 passed |
| cursor_viewport_pixels | 2 passed | 2 passed |
| inverse_pixels | 5 passed | 5 passed |
| inverse_default_color | 4 passed | 4 passed |

即"1 LSB 门禁"等既有阈值在软件适配器下同样满足，**没有放宽任何阈值**。

## 5. 2026-11-15 真实 Windows GPU 测试结论（Windows 侧数据，已验证事实）

- 机器：AMD Ryzen 5 5600U 核显；Vulkan 与 DX12 均可见，选中 Vulkan 适配器；
  GPU init 140–191ms；GPU 与 CPU 在共有文字行上逐像素最大差 1 LSB；
  GPU 首帧中位 325/356ms，CPU 359ms —— **GPU 路径真实可用**。
- 未验证：DX12 与 Vulkan 的 present 差异、无 GPU 机器上的退出路径、WARP 行为。
- **Windows 的 `auto` 默认仍为 CPU**；翻转条件：若 `backend="gpu"` 在多机多驱动下 DX12 与
  Vulkan 的 present 都稳定，可把 auto 改为"先试 GPU"。

## 6. Windows 3Q 交接（WARP 验证清单）

1. **无 GPU 机器用 WARP 验证 GPU 路径**：设 `APP_GPU_ALLOW_SOFTWARE=1` + `backend="gpu"`，
   应看到用户可见警告与 `software=true`，且窗口正常出画（WARP 即 DX12 的软件适配器）。
2. **`WGPU_BACKEND` 严格解析**：在 Windows 上分别用 `WGPU_BACKEND=dx12`、`WGPU_BACKEND=vulkan`、
   以及故意带尾随空格的形式，检查 `backends_env='…'` 与 `backends_effective=…` 是否一致；
   用 `WGPU_BACKEND=dx` 验证会报"含无法识别的后端记号"并非零退出。
3. **空闲不再空转**：`backend="gpu"` + `APP_PERF_FRAMES=1`，输出后静置，确认
   `frames_window` 收敛到 0（WARP 上同样应成立；若 WARP 下 present 有额外行为，请记录）。
4. **`auto` 忽略软件开关**：Windows 上 `auto` 应始终 CPU，日志里不应出现 `gpu_adapters`。
5. **DX12 present**：`WGPU_BACKEND=dx12` 下窗口内容、resize、最小化恢复是否正常。

## 7. 未解决问题

1. **Windows 上未复测**（本任务全部在 Linux 完成）；DX12/WARP 的实际行为仍未核实。
2. **空闲修复只加在 GPU 路径**：CPU 后端空闲时仍会"渲染但不提交"（每帧白算，但 CPU 成本低）。
   若将来 CPU 侧的 `snap.is_clean()` 判定同样安全，可一并提前返回（未做，避免扩大改动）。
3. **`WGPU_BACKEND` 平台可用性检查**依赖"枚举为空 → 报错"，没有在解析阶段区分"该后端在本构建
   未编译"，错误文案已提示这一点但未进一步细分。

## 8. 手动清单（≤3）

1. Windows 上用 WARP（`APP_GPU_ALLOW_SOFTWARE=1` + `backend="gpu"`）验证 GPU 路径能出画。
2. Windows 上验证 `WGPU_BACKEND=dx12` / `vulkan` 的 `backends_effective` 与 present 行为。
3. 真实使用中观察 GPU 后端空闲后的功耗/帧计数，确认不再持续渲染。
