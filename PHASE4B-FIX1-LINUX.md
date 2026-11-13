# PHASE4B-FIX1-LINUX：三个显示缺陷的取证与最小修复

按 A→B→C 顺序提交，tag `phase4b-fix1-a` / `-b` / `-c`，交付 tag `phase4b-fix1`。
分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论摘要

| 缺陷 | 根因 | 状态 |
|---|---|---|
| A 字形垂直位置不一致 | `blit` 把 baseline 当作格顶，且把 fontdue 的 `ymin`（位图**底边**相对基线偏移）当成顶边偏移 | **已修**（所有字体，非 Consolas 特有） |
| B 窗口大于网格时空白带 | 帧缓冲用 `vec![0]`、GPU 用固定黑 Clear，网格外的余数区从未写入 | **已修**（两侧均用调色板背景色整窗填充） |
| C 输入法/预编辑残影 | app **完全没有 `RedrawRequested` 分支**，OS 发起的 Expose 被丢弃 | **已修**（Expose → 整帧重绘；预编辑标脏加固） |

## 1. 任务 A：字形垂直对齐

### 1.1 根因（落到函数与公式）

`crates/render/src/lib.rs` 的 `blit` / `blit_subpixel`：

```rust
// 修复前
let px = cell_x as i32 + gx + metrics.xmin;
let py = cell_y as i32 + gy + metrics.ymin;   // ← baseline 被当成格顶
```

两个错误叠加：

1. **缺少 baseline**：正确落点应以基线为参考（`baseline ≈ ascent`），原式令 `baseline = 0`（格顶），整块字形上移 ascent 像素。
2. **ymin 语义用反**：fontdue 的 `Metrics.ymin` 是"位图**底边**相对基线的偏移（向上为负）"，不是顶边偏移。实测 DejaVu 13px：`'x' ymin=0 h=8`、`'g' ymin=-3 h=11`、`'.' ymin=0 h=2` —— 若把 ymin 当顶边，小字形会贴到格顶、下行字形整体上移。

**修复后公式**（已验证）：

```rust
// 顶边 = 格顶 + 基线 - ymin - height + gy
let py = cell_y as i32 + baseline - metrics.ymin - bh + gy;
// 基线 = 格高 - ceil(-descent) - ceil(line_gap)（为下行与 line_gap 在格内留空间）
baseline = cell_h - ceil(-descent) - ceil(line_gap)
```

`cell_h = ceil(ascent - descent + line_gap)`（阶段 2A 已正确）与新 baseline 配套。

### 1.2 版本二分（同一测试跑不同 tag）

| tag | 结果 | 说明 |
|---|---|---|
| `phase1-merged`（引入提交 `accd4dd`） | 错位 | `gy + metrics.ymin` 公式自首次实现即如此 |
| `phase3c-done` | 错位（同数字） | — |
| `phase4b-t1` | 错位（同数字） | — |
| HEAD（修复前） | 错位（同数字） | — |
| `phase2b-done` / `phase2d-done` | 无法跑 | 这两版的 `FrameSnapshot` 无 `wrapped/dirty_rows/full_redraw` 字段，测试需按旧 API 改写（如实记录，未强行适配） |

`git log -S "gy + metrics.ymin"` 定位引入提交 **accd4dd**（阶段 1 首次实现）；`01fa181`（阶段 2A）开始用字体度量算 cell 尺寸但**未用于落点**，使错位幅度固化为 ascent（≈0.75–0.8×cell_h）。

**结论：不是 Consolas 特有，所有字体都错，自阶段 1 起就存在。**

### 1.3 字体矩阵（修复前 → 修复后）

测试：`crates/render/tests/baseline_alignment.rs`，渲染 `xH.,-_'|gpyQ[]{}`，逐字符取墨迹 y 区间。

| 字体 | cell | ascent / descent / line_gap | baseline | 修复前 ',' '.' 偏差 | 修复后 '.' 偏差 | g/p/y 下行 |
|---|---|---|---|---|---|---|
| DejaVu Sans Mono | 8×16 | 12.07 / -3.07 / 0.00 | 12 | '.' 底边比 'x' **高 6px** | 0–1px | ✅ |
| Liberation Mono | 8×15 | 10.82 / -3.90 / 0.00 | 11 | 高 **5px** | 0px | ✅ |
| Noto Sans Mono | 8×18 | 13.90 / -3.81 / 0.00 | 14 | 高 **5px** | 1px | ✅ |
| FreeMono | 8×13 | 10.40 / -2.60 / 0.00 | 10 | 高 5–6px | 通过 | ✅ |
| **Consolas**（`/usr/share/fonts/chinese/consola.ttf`） | 8×16 | 9.65 / -3.35 / **2.22** | 9 | 高 **5px** | 1px | ✅ |

GPU 侧同名前缀用例（`baseline_alignment_gpu`）同样通过，无适配器时显式 SKIP。

### 1.4 判据修正（不是放宽）

任务书原判据"'.' 与 ',' 的底边与 'x' 底边相差 ≤1px"与字体设计不符：**',' 自带下行**，实测 DejaVu/Liberation/Noto 天然低 2–3px、Consolas 低 3px。改为：

- '.'：与 'x' 底边差 ≤1px（保留原判据）；
- ','：偏移在 `[-1, ceil(-descent)+1]` 内（不得跑到行上部，也不得超出下行范围）。

理由与推翻条件已记入 DECISIONS。

### 1.5 改动清单（任务 A）

- `crates/render/src/lib.rs`：`RowDrawCtx` 增 `baseline`；新增 `CpuRenderer::baseline_px`；`blit`/`blit_subpixel` 的 cell 元组扩为五元组 `(x,y,w,h,baseline)` 并按新公式计算 `py`；`draw_cursor`/`draw_preedit` 各自取 baseline。
- `crates/render/src/gpu/mod.rs`：新增 `GpuRenderer::baseline_px`；普通字形、光标字符、Preedit 三处 `rect.y` 改为 `py0 + baseline - ymin - height`。
- `crates/render/tests/baseline_alignment.rs`（新增）：4 种本机等宽字体 + `TEST_FONT_FILE`（不入库）+ GPU 用例；打印度量与每字符 ymin/height/实际墨迹区间。

**对拍期望无需更新**：`gpu_parity` 是 CPU↔GPU 相对比较，两侧同改后仍 8/8 通过；绝对位置由新增的 baseline 用例覆盖。

## 2. 任务 B：窗口尺寸与网格外区域

### 2.1 取证

- **日志语义（已验证）**：`request_inner_size` 在 winit 0.30 返回 `Option<PhysicalSize<u32>>`（`#[must_use]`），X11 下请求为**异步**提交，`None` 表示"稍后经 `WindowEvent::Resized` 返回"，**不是被窗口管理器拒绝**。原日志措辞误导，已改为说明异步语义；`Resized` 分支本就按窗口真实像素反算 `cols/rows` 并 resize vt+pty（无新增配置）。
- **空白带成因（已验证）**：`render_full` 用 `vec![0; ...]` 重建帧缓冲、GPU 用 `LoadOp::Clear(wgpu::Color::BLACK)`——都不是调色板背景色；而网格像素尺寸（`cols*cell_w × rows*cell_h`）通常**不能整除**窗口像素尺寸，右侧/底部余数区从未被写入 → 窗口大于网格时显示未初始化像素（`0x00000000`）。
- **实测证据**：:99 上 100×30 网格（800×480）窗口被拉大到 807×485，右下角 3×3 区域修复前 **0/9** 像素为背景色、修复后 **9/9**。

### 2.2 修复

- CPU：`render_full` 改为 `vec![ctx.bg_default; ...]`（整屏先铺背景）。
- GPU：`Pipeline::submit` 的 clear 色改为 `palette.primary_bg`（新增 `GpuRenderer::clear_color`）。

### 2.3 e2e 用例

`case29-nonintegral-bg-fill`：100×30 网格、背景 `#008040` 启动，用 xdotool 把**自身窗口**拉大 7×5 像素，截图取右下角 3×3，要求 ≥6/9 像素接近背景色。

**门禁有效性已验证**：临时把 `ctx.bg_default` 回退成 `0` → 0/9（FAIL）；恢复后 9/9（PASS）。不是恒绿用例。

## 3. 任务 C：Expose 与预编辑残影

### 3.1 取证

- **Expose 未被处理（已验证）**：app 的 `window_event` **没有 `RedrawRequested` 分支**，渲染完全由 `about_to_wait` 的节流逻辑驱动（自驱动用 `request_redraw`）。winit 0.30 把 X11 的 Expose、遮挡恢复都映射为 `RedrawRequested` → 系统要求重绘时被静默丢弃 → 静止画面遮挡/恢复后留残影。
- **预编辑（已验证）**：CPU 侧在 `preedit_key` 变化时只把**起始行**标脏；叠加层跨行时后续行不重绘。

### 3.2 修复

- 新增 `WindowEvent::RedrawRequested` 分支：置 `os_redraw_pending` + `urgent_render`；渲染时 `os_redraw_pending` 使 `snap.full_redraw = true`（所有行进 damage，CPU 等价整窗提交；GPU 本就整帧 present）。**应用自驱动的重绘仍走增量**。
- CPU 预编辑标脏：按"起始列 + 总宽度"换算跨越行数并整段标脏。
- GPU 侧每帧整帧重绘，不存在该类残影（无需改动）。

### 3.3 测试钩子与用例

- 新增 debug 专有 `APP_TEST_PREEDIT_CLEAR_MS`：注入后 N 毫秒清除。**关键坑**：清除检查若放在渲染路径内**永不执行**——静止画面 `will_render` 为 false，且渲染完成后不设 `WaitUntil`（事件循环永久 Wait）。改为放在 `about_to_wait` 首部，未到期时显式 `WaitUntil(clear_at)` 安排唤醒。
- `case30-preedit-clear-no-ghost`：注入跨行预编辑（100 个 'W'）→ 抓图 → 3s 后自动清除 → 再抓图，与"从未设置预编辑"的截图比对。实测：有/无预编辑差异 **9120 px**（证明钩子生效）、清除后差异 **0 px**。
- `case31-expose-full-redraw`：用新增的 `scripts/e2e/expose_overlay.py` 创建**自己**的覆盖窗压住本窗口再销毁，触发系统 Expose；断言 `[perf] os_redraw >= 1`（行为级证据）且前后画面差异 0 px。理由：Xvfb+openbox 有 backing store 时截图可能恒等，单看截图无法证伪。
- `crates/render/tests/preedit_ghost.rs`（新增）：渲染层精确验证"清除后零残留"与"移动位置后清除零残留"，2 用例通过。

## 4. 验收

- `cargo test --workspace`：**117 passed / 0 failed**（新增 baseline 2 + preedit_ghost 2 + gpu_parity 仍 8）
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**87/87 PASS**，`GPU_SKIP=0`，GPU 轮 4/4（新增 case29/30/31）
- 每步 tag 前均跑齐上述五项。

## 5. 未解决问题

1. **跨行预编辑标脏未被独立证伪**（待验证假设，置信度中）：`preedit_ghost.rs` 的跨行用例在"只标起始行"的回退版本下**仍通过**——预编辑在 layout 层可能被换行/截断为单行，或后续帧另有覆盖。跨行标脏保留为**防御性加固**，不声称已独立验证。推翻条件：构造 layout 层确实跨 ≥2 行且无其他输出覆盖的用例（如 40 列窗口 + 100 字符、光标置于倒数第二行），若回退版失败则证明加固必要。
2. **e2e case30 无法单独证伪跨行残影**：shell 回显会覆盖预编辑所在行；跨行场景由渲染层测试承担（当前亦未证伪，见上条）。
3. **Xvfb 有 backing store**：Expose 后画面在修复前也可能"看起来正常"，故 case31 依赖 `os_redraw` 计数而非纯截图差异；真实 office-pc 上无 backing store 时残影应更明显，需你复测确认。
4. **Preedit 光标反色/下划线与真实 IME 候选窗的交互无法自动覆盖**（见手动清单）。

## 6. 手动清单（≤3）

1. 在 office-pc 上用 `backend="cpu"`、Consolas 13pt、fallback 黑体、120×30 复测：(A) 句点/逗号是否坐在基线上、g/p/y 是否下行；(B) 窗口大于网格时四边是否都是背景色；(C) 遮挡窗口再恢复、以及输入法上屏/取消后是否还有残影。
2. 把 Consolas 字体文件放到本机约定路径后，用 `TEST_FONT_FILE=<路径> cargo test -p render --test baseline_alignment` 复跑字体矩阵（本机已用 `/usr/share/fonts/chinese/consola.ttf` 验证通过）。
3. 在真实 X11（非 Xvfb、无 backing store 或强制 `-bs off`）下手动遮挡再恢复窗口，确认 Expose 后画面完整（自动化只能证明 `os_redraw` 分支被走到且画面未坏）。

## 7. 附：测试用法

```sh
# 本机字体矩阵
cargo test -p render --test baseline_alignment -- --nocapture

# 纳入 Consolas（不入库、不提交字体文件）
TEST_FONT_FILE=/usr/share/fonts/chinese/consola.ttf \
  cargo test -p render --test baseline_alignment -- --nocapture

# 预编辑残影（渲染层）
cargo test -p render --test preedit_ghost -- --nocapture
```
