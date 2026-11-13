# PHASE4B-LINUX：wgpu(Vulkan) GpuRenderer 正式实现（Linux 专属；Windows 仍只编 CpuRenderer）

按 t1…t7 顺序完成，每步单独提交并打 tag。分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论摘要

| 项 | 结果 |
|---|---|
| 后端选择 | `renderer.backend = "auto"｜"cpu"｜"gpu"`，默认 auto；`auto` 优先 GPU、失败回落 CPU；`gpu` 失败非零退出 |
| GPU 与 CPU 像素等价 | 对拍门禁 **8 用例全过**：字形区最大通道差 **1 LSB**、差异占比 2.6–3.4%、CJK 压力帧 **0 差异**、Preedit 三种叠加 **0 差异** |
| e2e | **84/84 PASS**（CPU 轮 80 + GPU 门禁 1 + GPU 轮 4），`GPU_SKIP=0` |
| 启动 | GPU 首帧中位 **531ms** vs CPU 207ms；`gpu_init_ms` 中位 **307ms**（增加量几乎全是 GPU 初始化） |
| 吞吐（2M 行） | 120x30 **3666ms** vs CPU 4679ms；200x60 **5917ms** vs CPU 8407ms（同轮对比快 22%/30%） |
| :0 真机冒烟 | 通过（授权运行一次）：GPU 真实生效、截图成功、无残留 |
| Windows | 依赖树**零 wgpu/pollster**（97 crate vs Linux 205），`gpu` 回落 CPU 并警告 |

## 1. t1 宽字符取证（先于一切 GPU 工作）

**结论：CPU 侧确有截断缺陷，已单独提交修复。**

证据（`crates/render/tests/wide_glyph.rs`，直接读帧缓冲逐列统计）：

- 修复前：`回` 在 NotoSansCJK 16px 下位图 14×15、非零 alpha 列 **1..14**，但 CpuRenderer 渲染出的墨迹只到**列 9**、续格 **0 像素**。
- 三个对照组同时复现，排除了字体选择与裁剪范围：① 手工构造 `FrameSnapshot`（绕过 vt）；② 把 CJK 字体设为**主字体**（cell 9×24）；③ 插桩打印 `span=2 / area_w=20 / font_idx=Some(1) / s=1.0`（说明宽字符分支与裁剪都正确）。
- 根因：宽起始格按 `area_w=2 格` 铺背景并画字形后，**SpacerTail/SpacerHead 续格又按自己的单格 `fill_rect` 背景**，把字形右半擦成背景色。
- 修复（最小改动）：续格跳过 `fill_rect`（`if !is_tail { fill_rect(...) }`）。
- 修复后：墨迹列 **1..14**、续格 **45 像素**；1x / 2x / 200x60 / 手工构造 4 个用例全部通过。
- **4A 的"CPU 只画起始单格"结论作废**；4B 的 GPU 按 2 格裁剪，与修复后的 CPU 一致。

## 2. 各步改动清单

| 步 | 提交 | 改动要点 |
|---|---|---|
| t1 | 911e126 | `crates/render/src/lib.rs` 续格不再填背景；新增 `tests/wide_glyph.rs`（4 用例） |
| t2 | 579c8be | config 新增 `[renderer] backend`（默认 auto，非法值带路径警告回落）；app 新增 `RendererBackend` 枚举 + `create_renderer`（CPU 专有方法由枚举转发，**未给 `Renderer` trait 新增任何方法**）；示例配置补注释 |
| t3 | cad271b | `render::gpu`（cfg linux）：实例化 quad + R8 动态图集 + 离屏回读；字体组抽 `pub(crate) build_font_groups`；决策抽纯函数 `decide`；适配器过滤 `DeviceType::Cpu`；新增 `tests/gpu_parity.rs`；run_all 加 GPU 门禁并统计 skip |
| t4 | be888fb | `new_surface/resize_surface/render_surface`（winit raw handle、整帧重绘、不回读）；pipeline 格式取 surface 实际格式；Lost 重建 surface、Outdated reconfigure 后重试本帧；app 侧 GPU 不建 softbuffer、`fatal` 字段 + 非零退出；启动复用首个 CPU 渲染器（修掉 case11 回归） |
| t5 | cc5abfb | GPU Preedit 叠加（含组合光标反色与下划线）；对拍新增 preedit 用例（8 例）；新增 `scripts/e2e/run_gpu_round.sh` 与 run_all 汇总接入 |
| t6 | 028a8a2 | 性能实测数据与能力边界记录（P4 无法在正式路径指定；回显延迟无法用 dump 法测） |
| t7 | 93085a9 | `scripts/e2e/gpu_smoke_display0.sh`（ALLOW_DISPLAY0 门控）+ lint 白名单 |

## 3. 对拍门槛（t3/t5）

`cargo test -p render --test gpu_parity`（**8 用例，本机全部真实运行、0 SKIP**）：

| 用例 | 数据 |
|---|---|
| 120x30@1x | bg_diff 5/555103（0.001%）、glyph_diff 674/20897（3.23%）、glyph_max **1** |
| 120x30@2x | bg_diff 21、glyph_diff 1961/72476（2.71%）、glyph_max **1** |
| 200x60@1x | bg_diff 5、glyph_diff 674/22817（2.95%）、glyph_max **1** |
| 200x60@2x | bg_diff 21、glyph_diff 1961/76316（2.57%）、glyph_max **1** |
| 四种光标（Block/Hollow/Underline/Bar） | all_max **0** |
| Preedit（ASCII / CJK / 带 cursor_range） | all_max **0** |
| 最小反色复现 | all_max **0** |
| 图集压力（3000 个不同 CJK） | glyph_diff **0/443262**、glyph_max **0**、2048² 容纳 2950 字形、grows=0 |

门禁阈值（未放宽）：背景区差异 ≤0.5%、字形区最大通道差 ≤1 LSB、字形区差异占比 ≤8.3%（4A 实测最大值 4.15% 的 2 倍）。无适配器时显式打印 `SKIP:` 并计入 run_all 的 `GPU_SKIP`（当前 0）。

## 4. e2e 双后端（t5）

- **CPU 轮**：80 个用例（含 case1–28、release 零告警门禁、lint）。
- **GPU 门禁**：`PASS gpu-parity (passed=8 skip=0)`，计入 1。
- **GPU 轮**（`scripts/e2e/run_gpu_round.sh`，4 项全过）：
  1. `gpu-round-smoke`：backend=gpu 真用 GPU（gpu_init_ms + backend=gpu + first_frame_ms）；
  2. `gpu-round-screenshot-threshold`：GPU 与 CPU 同场景截图 `compare -fuzz 3%` → **0 像素差异**；
  3. `gpu-round-auto-fallback-cpu`：auto + `VK_ICD_FILENAMES=/nonexistent` → 打印回落日志且正常出首帧；
  4. `gpu-round-gpu-noicd-exit`：gpu + 无 ICD → **exit=1**（非超时）。
- 汇总行：`===== e2e 汇总: PASS=84 FAIL=0 GPU_SKIP=0 [GPU 轮汇总: PASS=4 FAIL=0 SKIP=0] =====`

**能力边界（不是放过失败）**：`APP_DUMP_SNAPSHOT` 仅 CPU 后端可写（GPU 不回读），脏行增量语义在 GPU 后端不存在（每帧整帧重绘），因此 case1–11/19–22/28 等 dump 依赖用例**未在 GPU 轮重复执行**；GPU 与 CPU 的逐像素等价由上表的 cargo 门禁承担。

## 5. 性能对比（t6，release，:99）

### 5.1 启动（各 5 次）

| 后端 | gpu_init_ms | first_frame_ms |
|---|---|---|
| gpu | 293–345（中位 **307**） | 523–575（中位 **531**） |
| auto | 302–321（中位 **317**） | 521–555（中位 **527**） |
| cpu（对照） | — | 184–213（中位 **207**） |

**auto 模式增加的首帧延迟 ≈320ms，几乎全部是 GPU 初始化**。3C 记录的"无配置约 220ms"与本次 CPU 对照 207ms 一致。

### 5.2 吞吐（2,000,000 行满屏滚动 + shell 退出，墙钟同 3C 口径）

| 尺寸 | GPU | CPU（同轮） | 3C CPU 基线 |
|---|---|---|---|
| 120x30 | **3666ms** | 4679ms | 4736ms |
| 200x60 | **5917ms** | 8407ms | 6479ms |

### 5.3 帧统计与自适应间隔

| 后端/尺寸 | render_ms 中位 | interval 收敛 | coalesced |
|---|---|---|---|
| GPU 120x30 | **14**（已含 present） | 46ms（≈21fps） | 2634 |
| CPU 120x30 | 7（present 3） | 23ms（≈43fps） | 968 |
| GPU 200x60 | **44** | **100ms（触上限）**（≈10fps） | 1593 |
| CPU 200x60 | 23（present 10） | 70ms（≈14fps） | 1075 |

- GPU 单帧更贵，但整帧重绘省掉脏区拷贝与 damage 提交，且每帧合并更多 PTY 唤醒 → 总吞吐更好。
- framedpace 在 GPU 下按预期收敛（3×EMA 推算 42/46、132→clamp 100 与实测一致）；**200x60 已触 100ms 上限**，建议 4C 评估 X11 present 路径（减少整帧上传/用 shm），而不是调参。本轮未改任何 framepace 参数。
- **回显延迟**：无法用 3C 的 dump 时间戳法（dump 仅 CPU 后端）。以 urgent 帧下限 8ms + render_ms 估算上界：120x30 ≈22ms、200x60 ≈52ms（3C CPU 实测 25.36ms）——**估算，非实测**。

### 5.4 :0 真机冒烟（t7，授权一次）

`ALLOW_DISPLAY0=yes` 下运行 `gpu_smoke_display0.sh`：开头/结尾 `DISPLAY=:0`；自身窗口 `0x3800004`；`xwd` 截图 3.28MB；`gpu_init_ms=339.5 backend=gpu adapter='NVIDIA GeForce GT 710'`、`first_frame_ms=556`；3s 内不足 50 帧故无 frames 行。运行前后 openbox 2 个 / Xorg pid 1698 不变，无 `"app"` 窗口与进程残留。

## 6. 未解决问题

1. **CPU 反色缺陷仍未修**（置信度高，已验证事实）：`blit` 的混合底用填充背景色而非 inverse 交换后的 `bgc`，导致反色单元格渲染为"黑底黑字"。本阶段为保证 GPU 与 CPU 逐像素一致，GPU **复刻**了该行为；修复需同时改两侧并更新对拍期望值，建议单独立项。
2. **200x60 在 :99 上触到 100ms 帧间隔上限**（≈10fps）：GPU 单帧 44ms 主要来自整帧 surface 上传 + Xvfb 软渲染；真实 GPU 显示路径应更快，但未在 :0 做吞吐测量（t7 只做冒烟）。
3. **Tesla P4 未在正式路径验证**：app 未提供适配器选择配置（任务禁止新增未要求项），`Gpu::new` 取首个非 Cpu 适配器（GT 710）。P4 只有 4A 的离屏数据（submit 0.21–1.36ms）。
4. **GPU 后端无回读**：因此 dump 钩子、帧哈希对拍、回显延迟实测在 GPU 下均不可用；若要补齐需加 debug 专有回读钩子（编译期开关）。
5. **动态图集增长/清空路径未被真实触发**（当前 grows=0、resets=0）：压力用例 3000 字形在 2048² 内放得下；增长与清空逻辑虽实现并测试通过编译，但**未在真实溢出场景验证**（待验证假设）。推翻条件：用 >12000 个不同 CJK 字符或更大字号触发增长，画面仍应与 CPU 参照一致。

## 7. Windows 交接（三项确认）

1. **cfg 后仍零 wgpu 依赖（已实测）**：`cargo tree --target x86_64-pc-windows-gnu -e normal` 中无 `wgpu`、无 `pollster`（唯一相关项是 winit 自带引入的 `raw-window-handle`）；Windows 目标 crate 总数 **97** vs Linux **205**。wgpu 只写在 `crates/render/Cargo.toml` 的 `[target.'cfg(target_os = "linux")'.dependencies]`。
2. **backend=gpu 回落警告**：`create_renderer` 的 `cfg(not(target_os = "linux"))` 分支打印 `[renderer] backend=gpu 在当前平台不可用，回退 cpu` 并回落 CPU（auto 在 Windows 也直接 CPU，不尝试 GPU）。
3. **run_all 不变**：CPU 轮与既有 80 用例未改动（仅把默认配置显式设为 `backend="cpu"`，因为默认已改为 auto）；新增的 `run_gpu_round.sh` 依赖 Linux 的 Xvfb/Vulkan/`compare`，Windows 侧若跑 run_all 应跳过该轮（它不参与 PASS/FAIL 判定以外的任何断言）。

**Windows 侧需验证**：① `cargo build --target x86_64-pc-windows-gnu` 通过且 `cargo tree` 无 wgpu；② `backend="gpu"` 启动打印回落警告且窗口正常；③ `cargo test -p render` 中 GPU 相关测试被 `#![cfg(target_os = "linux")]` 排除（`gpu_parity.rs` 与 `wide_glyph.rs` 首个文件级 cfg）。

## 8. 手动清单（≤3）

1. 在真实 GPU 的 X 会话（非 Xvfb）用 `backend="gpu"` 跑 2M 行吞吐与 200x60 观感，确认帧间隔不再触 100ms 上限。
2. 人眼对比同场景 `backend=cpu` 与 `backend=gpu` 的显示（尤其反色、下划线、选区、Preedit），确认无可见差异。
3. 在有 Vulkan 的 Windows 机器上确认 `backend="gpu"` 回落日志与窗口正常（Windows 本轮不提供 GPU 后端）。

## 9. 验收

- `cargo test --workspace`：**113 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**84/84 PASS**，`GPU_SKIP=0`，GPU 轮 **4/4**
- 各步 tag：`phase4b-t1`…`phase4b-t7`；交付 tag `phase4b-done`（hash 见交付说明）
