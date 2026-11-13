# PHASE4A-LINUX：GPU 渲染后端选型验证（spike）

只验证与出报告，**未接入正式代码路径**。分级：已验证事实 / 工程判断（附置信度）/ 待验证假设（附推翻条件）。

## 0. 结论摘要

| 项 | 结论 |
|---|---|
| 推荐后端 | **wgpu（Vulkan）**，理由与推翻条件见 §5 |
| 与 CPU 参照像素一致性 | wgpu：字形区最大通道差 **1 LSB**，背景除舍入外一致（diff 0.27%）；GL：背景 0.036% 差异、字形区未对齐（见 §3.3） |
| 渲染耗时（不含回读，300 帧中位） | wgpu/GT 710：1.31–7.54ms；wgpu/Tesla P4：0.21–1.36ms；CPU 同快照全量重绘：3.2–42.5ms |
| 无 GPU 时 | 适配器枚举返回 Err（21ms），不 panic；可安全回落 CPU |
| :99 下 present | lavapipe 与 NVIDIA 均 10/10 帧成功；`:0` 被程序拒绝 |
| 部署 | 两者都自包含（ldd 仅 libc/libm/libgcc_s），GPU API 运行时 dlopen |

## 1. T0 环境（已验证事实）

- GPU：`NVIDIA GeForce GT 710`（GK208B，PCI 06:00.0）、`Tesla P4`（GP104GL，07:00.0）
- 驱动：NVIDIA 470.256.02；`vulkaninfo --summary` 报 **Vulkan API 1.2.175**（instance 1.3.239）
- `/dev/dri`：card0、card1、renderD128、renderD129
- lavapipe ICD 存在：`/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`
- CPU/OS：Ryzen 5 2600（12 线程）、Devuan 5（glibc 2.36）；rustc 1.99.0，主 workspace MSRV 1.96
- 图形库：EGL 1.5、`/usr/include/vulkan/vulkan.h`、`libEGL_mesa`/`libEGL_nvidia` 均在位

真实物理 Vulkan 设备存在 → 继续，未使用 lavapipe 数据替代真实 GPU 数据。

## 2. spike 隔离与依赖

- 目录 `gpu-spike/`，自带 `[workspace]`（members = `common` / `src-wgpu` / `src-gl`），以 path 依赖**只读**引用主仓库 `crates/vt`、`crates/render`。主 workspace members 与主 `Cargo.lock` 未改动；主仓库 `cargo test`（98 测试）/`clippy`（0）/`run_all`（82/82）不受影响。
- 版本（Cargo.lock 实测）：wgpu **30.0.2**（default-features=false，features=`vulkan`,`wgsl`；声明 MSRV 1.87，在 rustc 1.99 编译通过）、naga 30.0.2、glow 0.18.0、khronos-egl 6.0.0、x11-dl 2.21、raw-window-handle 0.6.2、fontdue 0.9.5、fontdb 0.23。
- 两侧共用 `spike-common` 的：固定测试帧（中英文/样式/光标/选区）、字体图集（四组 bold/italic × 回退链）、**实例数据构建**（完整复刻 CPU 的绘制语义）、像素对比、统计。因此两条 GPU 路径的 CPU 侧输入完全相同。

### wgpu 30 API 事实（踩坑记录，已验证）

`Instance::new(InstanceDescriptor)` 按值且无 `Default`；`request_device`/`enumerate_adapters` 返回 Future；`AdapterInfo` 无 `apiVersion`；`Device::poll(PollType::Wait{submission_index,timeout})`；`RenderPassDescriptor.multiview_mask`；`PipelineLayoutDescriptor.immediate_size`（不是 push_constant_ranges）；`bind_group_layouts` 元素为 `Option<&BindGroupLayout>`；顶点属性布局需 `'static`；`SurfaceConfiguration.color_space`、`RequestAdapterOptions.apply_limit_buckets`、`get_current_texture()` 返回 `CurrentSurfaceTexture` 枚举、present 在 `queue.present(texture)`。

## 3. 指标

采集条件：release；Xvfb 不参与（离屏渲染，不需要任何显示）；每配置 300 帧**全量重绘**；时间分三段——CPU 构建实例、提交到 GPU 完成（submit + `poll(Wait)`）、回读（copy + map / readPixels）。中位/p95，单位 ms。

### 3.1 wgpu（Vulkan）

| GPU | 尺寸/缩放 | 像素 | build | submit+wait | readback | CPU 全量重绘 |
|---|---|---|---|---|---|---|
| GT 710 | 120x30 @1x | 1200x480 | 0.13 / 0.14 | **1.31 / 1.56** | 5.8 / 6.2 | 3.2 / 3.3 |
| GT 710 | 120x30 @2x | 2400x960 | 0.14 / 0.16 | **2.71 / 2.92** | 23.1 / 23.4 | 13.9 / 14.0 |
| GT 710 | 200x60 @1x | 2000x960 | 0.33 / 0.40 | **3.52 / 3.75** | 19.3 / 19.5 | 10.7 / 10.7 |
| GT 710 | 200x60 @2x | 4000x1920 | 0.33 / 0.49 | **7.54 / 7.86** | 76.7 / 77.1 | 42.5 / 42.8 |
| Tesla P4 | 120x30 @1x | 1200x480 | 0.05 / 0.06 | **0.21 / 0.23** | 0.3 / 0.4 | 3.2 / 3.3 |
| Tesla P4 | 120x30 @2x | 2400x960 | 0.06 / 0.07 | **0.47 / 0.49** | 1.9 / 1.9 | 13.9 / 13.9 |
| Tesla P4 | 200x60 @1x | 2000x960 | 0.14 / 0.18 | **0.63 / 0.67** | 1.6 / 1.7 | 10.6 / 10.7 |
| Tesla P4 | 200x60 @2x | 4000x1920 | 0.33 / 0.36 | **1.36 / 1.42** | 7.1 / 7.3 | 42.6 / 43.1 |

初始化（每次进程）：wgpu instance+adapter+device **121–207ms**、管线+资源 29–42ms；另有 **atlas 1.75–1.81s**——这是对比夹具成本（fontdb 扫描 + NotoSansCJK 19MB ttc 的 fontdue 解析），主程序已有 face 缓存与后台加载，**不计入 wgpu 开销**。

解读：**不含回读时，GPU 渲染比 CPU 全量重绘快 2.4×（GT 710 @120x30）到 31×（P4 @200x60@2x）**。回读是本 spike 的测量开销（GT 710 的 76.7ms 说明其 PCIe 读回很慢），真实终端呈现走 surface 不需要回读。

### 3.2 OpenGL ES（EGL surfaceless，llvmpipe）

| 尺寸/缩放 | build | draw+finish | readback | CPU 全量重绘 |
|---|---|---|---|---|
| 120x30 @1x | 0.07 / 0.10 | **3.8 / 5.0** | 0.55 / 0.69 | 3.3 |
| 120x30 @2x | 0.14 / 0.19 | **6.3 / 8.2** | 3.74 / 4.21 | 13.7 |
| 200x60 @1x | 0.16 / 0.25 | **10.0 / 13.5** | 2.99 / 3.38 | 10.8 |
| 200x60 @2x | 0.14 / 0.19 | **16.6 / 19.4** | 12.40 / 14.77 | 42.9 |

**注意：这是 llvmpipe 软件渲染**，不是 GPU。本机在无 DISPLAY 时 Mesa EGL 只提供 GLES 且只经 llvmpipe（NVIDIA 的 EGL 设备平台需要 `EGL_EXT_device_enumeration`，khronos-egl 6.0 未暴露该 API，本次未手写 dlsym）。因此该组数字只能说明"GL 路径可用"，**不能与 wgpu 的 GPU 数字直接比较**。

### 3.3 像素对比（与同一快照的 CpuRenderer 输出）

口径：CPU 帧缓冲 `0xAARRGGBB`；GPU 目标为 `Rgba8Unorm`（非 sRGB，避免色彩空间转换）；字形在 shader 内 `mix(backdrop, fg, coverage)`，与 CPU 的 `blend_gamma(fg,bg,a,gamma=1.0)` 语义等价。前景/背景按"CPU 像素是否等于默认背景色"划分。

| 实现 | 尺寸 | 差异像素/总数 | 背景区 | 字形区 | 最大通道差（全/字形） |
|---|---|---|---|---|---|
| wgpu | 120x30@1x | 1534 / 576000（0.27%） | 228 | 1306 / 31448 | 254 / **1** |
| wgpu | 120x30@2x | 4378 / 2304000（0.19%） | 656 | 3722 / 113174 | 254 / **1** |
| wgpu | 200x60@1x | 1534 / 1920000（0.08%） | 228 | 1306 / 31928 | 254 / **1** |
| wgpu | 200x60@2x | 4378 / 7680000（0.06%） | 656 | 3722 / 115094 | 254 / **1** |
| GL | 120x30@1x | 10274 / 576000（1.78%） | 206 | 10068 / 31448 | 254 / **254** |

- **wgpu 的字形区最大通道差 = 1 LSB**，即混合结果仅差整数截断 vs 浮点舍入；背景区差异同样为 1–2 LSB（CPU 截断到 0 而 GPU 为 1）。**未做任何断言放宽**。
- 差异来源（已验证事实）：① 整数截断 vs 浮点舍入（1 LSB）；② 宽字符的裁剪范围——CPU 侧 `blit` 的实际可见范围是**起始单格**，GPU 若按 2 格裁剪会让续格多出 5–10 列，故 GPU 也裁到单格以对齐参照实现（这**不是物理正确性**，登记为 4B 待办）。曾出现过的 21304 差异是夹具问题（CPU 未加载 CJK 字体画占位方块）与 shader 未对 coverage==0 `discard`（CPU 的 blit 不写该像素）两项，均已修正。
- **GL 路径未对齐**（见 §6 未解决问题），故 GL 的像素数据不能用于"GL 也能像素级对齐"的结论。

### 3.4 工程指标

| 项 | wgpu spike | GL spike | 主 app（参照） |
|---|---|---|---|
| `cargo tree -e normal` 唯一 crate 数 | **137** | **87** | 159 |
| release 二进制体积 | **10.89MB** | **4.27MB** | — |
| 冷编译（clean 后 release） | \multicolumn{2}{c}{**233s**} | — |
| `ldd` 动态依赖 | libc/libm/libgcc_s | libc/libm/libgcc_s | — |
| 运行期 GPU API 加载 | **dlopen** libvulkan.so.1（strace 证据）+ intel/radeon/lvp ICD | **dlopen** libEGL.so.1 + libEGL_mesa/libEGL_nvidia | — |
| 静态/自包含部署 | 可行（自包含，仅需系统 Vulkan loader + ICD） | 可行（需系统 libEGL + GL 驱动） | — |

## 4. T4 无 GPU 与 Xvfb 路径

- **T4a（已验证）**：`VK_ICD_FILENAMES=/nonexistent/icd.json` → 适配器枚举为空，返回 `Err("Vulkan 适配器枚举为空…")`，进程 exit=1、**无 panic**，耗时 **21ms**（3 次一致）。单独指向 lavapipe → 被识别为 `Cpu` 并跳过、返回 Err（设计如此，避免软件实现掩盖真实设备缺失）。→ "无 GPU 回落 CPU" 的前提成立。
- **T4b（已验证）**：`:99`（经 `scripts/e2e/guard.sh` 起 Xvfb，未触碰 `:0`）用 X11 surface（x11-dl + raw-window-handle 0.6）present：lavapipe **10/10 帧**（`Bgra8UnormSrgb`）、NVIDIA GT 710 **10/10 帧**。安全自检：`DISPLAY=:0` 的请求被程序直接拒绝。

## 5. 推荐与推翻条件

**推荐 wgpu（Vulkan）**（工程判断，置信度高，依据：实测数据 + 生态成熟度）：

1. **性能**：真实 GPU 上 submit+wait 1.31–7.54ms（GT 710）/ 0.21–1.36ms（P4），均低于 CPU 全量重绘，且 200x60@2x 仍有 5.6×～31× 余量。
2. **正确性**：与 CPU 参照达到 1 LSB 级一致；这是可验证、可回归的状态（本 spike 的对比工具可复用为 4B 的验收手段）。
3. **失败可检测**：无 ICD 时 21ms 内返回 Err，能干净回落 CPU；软件适配器（lavapipe）被显式过滤，不会静默降级。
4. **部署**：自包含二进制（仅 dlopen 系统 Vulkan loader），crate 数 137 与主 app 159 同量级，冷编译 233s 可接受。
5. **surface 路径已验证**：:99 下 lavapipe 与 NVIDIA 均能 present。

不选 GL 作为主路径（工程判断，置信度中）：本机无 DISPLAY 时只能拿到 llvmpipe（软件），无法验证真实 GPU 的 GL 性能；且本次 spike 的 GL 实现存在未对齐的字形偏差（§6），进一步投入的收益不确定。

**推翻条件**（任一发生则应重新评估）：
- 真实 GPU 的 GL 路径（有 X 时）在同等条件下 submit 耗时显著低于 wgpu，且字形能做到 1 LSB 对齐；
- wgpu 在目标部署环境（无 Vulkan ICD、旧驱动、受限容器）普遍不可用，而 GL 可用；
- 接入 4B 后，`present_with_damage` 等增量路径在 wgpu 上无法表达或成本过高（如每帧 surface 重建）；
- 目标机型（老 Intel 核显等）实测 wgpu 初始化 > 500ms 或渲染劣于 CPU。

## 6. 对 4B 的接口草案

**GpuRenderer 如何不改 trait**：现有 `Renderer` trait 只有 `render(&mut self, frame, pixel_width, pixel_height)` 与默认空的 `set_preedit`，签名足够——`GpuRenderer` 内部持有 surface/device/pipeline，`render()` 把 `FrameSnapshot` 转成实例数据（复用本 spike 的 `build_instances`）后提交；`set_selection`/`set_scrollbar`/`damage()` 等是 `CpuRenderer` 的固有方法而非 trait 成员，接 4B 时若需要统一，建议**新增 trait 默认方法**（不破坏现有实现），或让 app 层按后端分支调用——两种都不需要改签名。

**运行时选择与回落位置**：在 app 创建渲染器的位置（`App::new` 内 `CpuRenderer::new` 处）做 `try GpuRenderer::new() → OK 用 GPU，Err 记录一行日志并用 CpuRenderer`。依据：T4a 证明 Err 路径 21ms 且不 panic，回落成本可忽略。注意必须在**首帧前**决定，避免中途切换造成增量状态不一致（脏行/条带备份都在 CPU 渲染器内）。

**配置字段**：需要 **1 个** `renderer.backend = auto|cpu|gpu`（默认 `auto`=试 GPU 失败回落 CPU）。理由：① 用户可能要知道当前是否在用 GPU（排查字体/画面问题）；② 无 GPU 环境需要强制 cpu 以跳过 100–200ms 探测；③ 避免"自动探测"在异常驱动上反复踩坑。不新增其他字段（字号/字体/颜色都不受影响）。按项目惯例，该字段加在 config crate 的 `[renderer]` 段并带默认值。

**4B 必须先解决的两点**：
1. **宽字符跨格渲染**：本 spike 为对齐 CPU 而裁到单格。若 4B 要正确跨格，需同时改 CPU 侧（否则增量对比会持续偏差）——建议 4B 单独一个任务处理，并加对拍用例。
2. **surface vs 离屏**：正式路径应渲染到 surface（无回读）；`present_with_damage` 在 wgpu 上对应"只提交脏区 scissor + 整帧 present"，需确认 softbuffer 的 damage 语义与 wgpu surface 的差异（surface 每帧必须整帧 present，但可用 scissor 减少绘制）。

**Windows 的 cfg 处理建议**：
- wgpu 在 Windows 用 DX12/Vulkan：spike 只启用 `vulkan` feature，Windows 侧需额外启用 `dx12`（或 `gles`）。建议 feature 由 `cfg(target_os)` 决定，或统一启用 `["vulkan","dx12","gles"]` 以换取可移植性（代价是 crate 数与体积上升）。
- surface 创建：Windows 用 HWND（`RawWindowHandle::Win32`），与 X11 分支同构，建议把 surface 创建隔离到一个 `#[cfg]` 模块（`surface_x11.rs` / `surface_win32.rs`），其余共享。
- 现有 Windows 专属文件（`crates/pty/src/windows.rs` 等）不要动；GPU 相关新增文件放在 render 侧并按平台 cfg。

## 7. 未解决问题

1. **GL 字形未对齐**（置信度中，待验证假设）：ASCII 逐格对照显示字形底部/右侧若干行缺失，diff 10068/31448 字形像素。已排除翻转方向、图集 v 方向、精度（mediump→highp）、顶点 stride（修正 68→72 有改善但未消除）、cell 裁剪边界、gl_FragCoord 替代 varying。剩余可疑：llvmpipe 的 GLES instancing/顶点属性语义。**推翻条件**：在 NVIDIA 真实 GL 上下文（有 X 时）复跑同一对比，若差异消失则说明是 llvmpipe 特有。
2. **本机无 DISPLAY 时无法验证 NVIDIA 的 GL 路径**：需 `EGL_EXT_device_enumeration`（khronos-egl 6.0 未提供，需手写 dlsym 或换库）。
3. **Tesla P4 无显示输出**：可作为渲染设备验证性能，但实际部署机型应是 GT 710 或核显；P4 的数字只用于说明"渲染本身很快"。
4. **回读成本**（GT 710 最大 76.7ms/帧）只在 spike 的测量路径存在；正式路径应避免回读，4B 需以 surface present 计时替代。
5. **Vulkan 1.2.175**：wgpu 30 在本机可用，但更高版本的 wgpu 若要求 Vulkan 1.3 特性，将无法在 470 驱动上运行——接 4B 时需锁版本并回归。

## 8. 手动清单（≤3）

1. 有真实 GPU 的 X 会话（非 Xvfb、非 llvmpipe）下复跑 GL spike，确认 §7.1 的字形偏差是否消失。
2. 人眼确认 wgpu 在 GT 710 上的 200x60@2x（4000x1920）画面无撕裂/闪烁（spike 只做离屏 + :99 surface 冒烟）。
3. 在目标部署机型（非本机 Tesla P4）上复测 wgpu 初始化耗时与首帧，确认 < 500ms。

## 9. 验收

- `gpu-spike` 独立 workspace：`cargo test --workspace` **4 passed / 0 failed**；`cargo clippy --workspace --all-targets -- -D warnings` **0**；`cargo clippy --release ...` **0**；`cargo build --release` **0 warning**。
- 主 workspace 未改动：`cargo test --workspace` 98 passed / 0 failed、clippy（debug/release）0、`cargo build --release` 0 warning、`scripts/e2e/run_all.sh` 82/82 PASS。
- 提交 `3d786b1`（T1，tag `phase4a-t1`）；T2–T4 与本文档在后续提交（tag `phase4a-done`）。
- `git ls-remote ali` 的实际 main hash 见交付说明（本地与远端一致）。
