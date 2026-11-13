# PHASE4C-LINUX：合并 Windows / 最小化守卫 / 观测增强 / 反色修复

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 Windows | tag `phase3d-win-done` 实测 = **4a2d83d**（merge 提交 415062a，无冲突）；shared 改动仅 `resize_surface` 的 cfg 参数处理 |
| t2 最小化守卫（P1） | 抽出 `resize_target()`；不足一格返回 None；**证伪已验证**（回退后单测红） |
| t3 观测增强 | `feed_bytes_per_s` / `chan_depth_max` / `renderer_ready_ms` / `pre_first_submit_ms`；自洽核对通过 |
| t4 反色缺陷 | 抽出 `decide.rs` 作唯一决策源；反色格底色与字色正确交换；**证伪已验证**（回退后 3 用例红） |

## 1. t1 合并 Windows

- **两个 hash 的实测值**（`git ls-remote ali`）：
  - `refs/tags/phase3d-win-done` = `27ee9f9755df1f6e11479cf4086a6f94e87482c0`（tag 对象），解引用 `^{}` = **`4a2d83d4e3b0365c33c31634eefc7daa02b300bc`**
  - `refs/heads/win` = **`4a2d83d4e3b0365c33c31634eefc7daa02b300bc`**（与 tag 指向同一 commit）
  - Windows 报告里出现的 `dfa882a` 是 win 分支上"记录 push/tag 验证"那条提交的描述文本，**不是 tag 指向的 commit**；以实测 4a2d83d 为准。
- merge-base = `b7aad4e`（4B-fix3 收口），`git merge --no-ff` 无冲突 → 合并提交 **415062a**。
- **shared 代码范围核对**（该 tag 相对 merge-base）：仅
  `crates/app/src/lib.rs` 的 `resize_surface` 增加
  `#[cfg(not(target_os = "linux"))] let _ = (width, height);`（4 行），
  另 `.gitignore` 增加两行 Windows 产物路径。其余 214 个文件全在
  `vt-verify-win/`、`scripts/windows/`、`docs/WIN-NOTES.md`、
  `crates/pty/tests/pty_windows.rs`、`PHASE*WIN.md` 等 Windows 专属路径。
  与预期一致，未超出 a579c36 的范围。
- Windows 目标依赖树复核：`cargo tree --target x86_64-pc-windows-gnu -e normal` 中
  `wgpu`/`pollster` 计数 **0**。

## 2. t2 最小化/零尺寸 resize 守卫（P1）

**根因**：`WindowEvent::Resized` 处理器用 `.max(1)` 兜底后无条件 `vt.resize`/`pty.resize`。
Windows 最小化实测发 1×1（有时 0×0）→ `w / cell_w == 0` 被抬到 1 → 网格变 1×1、
内容被裁且恢复后不可还原（vt 已 reshape，强制整帧重绘也无效）。

**守卫判据**（工程判断，置信度中）：

```rust
pub fn resize_target(w, h, cell_w, cell_h, pad_x, pad_y) -> Option<(u16, u16)>
// 宽或高不足一个完整字符格（w/cw == 0 或 h/ch == 0）→ None
```

依据：格子尺寸由字体度量决定（典型 8×16 / 10×16），1×1 窗口必然 `1/8 == 0`；
只判"尺寸为 0"拦不住 Windows 的最小化。None 时：不调 vt/pty resize、不重配
CPU/wgpu surface（wgpu 对 0 尺寸 configure 会 panic）、不改 `cols/rows/last_size`，
仅请求一次重绘。恢复时若行列与最小化前相同则跳过 vt/pty resize（幂等）。
顺带补上原先**缺失**的 `self.cols/self.rows` 更新（旧代码只改 vt，`pixel_to_cell` 用陈旧值）。

**门禁与证伪**（已验证）：`crates/app/tests/keyboard.rs` 新增 2 个单测，覆盖
(0,0) / (0,h) / (w,0) / (1,1) / 小于一格 / 恰好一格 / 正常尺寸 / padding 吃满 /
超大钳位 / 恢复幂等。**把 `resize_target` 回退成 `.max(1)` → 两用例均红**
（`Some((1,1))` vs `None`），恢复后绿。

**e2e 边界（如实记录）**：:99 下 `xdotool windowminimize` + `windowmap`
**不产生 `WindowEvent::Resized`**（日志无 `resize_ignored`/`vt resize`，cols/rows 不变），
故 case33 恒绿、**不能证伪守卫**，仅作回归保护。

**Windows 3E 待办**：最小化 → 恢复的真实复验（需 Windows 会话）。

## 3. t3 观测增强（不改行为）

- `APP_PERF_FRAMES` 每 50 帧日志新增：
  - `feed_bytes_per_s`：本窗口喂入 vt 的字节数 / 窗口时长；
  - `chan_depth_max`：有界 channel 积压峰值（`mpsc::Receiver` 不暴露 len，改用
    `Arc<AtomicUsize>`：reader 入队 +1、主线程取走 −1；新增 `reader_body_with_depth`，
    `reader_body` 保留为转发，既有测试无需改）。
- 启动分段新增 `renderer_ready_ms`（渲染器与字形缓存初始化完成）与
  `pre_first_submit_ms`（首帧提交前一刻），与 `first_frame_ms` 一并输出。
- release 可用、默认关闭路径只有一次 `Option` 判空；未调整任何 framepace 参数。

**实测（release，:99，2M 行 120x30，3 次）**

| run | 墙钟 | 末窗口 feed_bytes_per_s | chan_depth_max | 窗口速率区间 |
|---|---|---|---|---|
| 1 | 3267ms | 54.5 MB/s | 1 | 30.0–54.5 MB/s |
| 2 | 4640ms | 34.4 MB/s | 1 | 20.2–34.4 MB/s |
| 3 | 3745ms | 39.3 MB/s | 1 | 34.2–39.3 MB/s |

**自洽性核对**：ticker 每行 53 字节（含 ONLCR）→ 总字节 ≈ **106 MB**；
全程平均速率 = 106MB /（墙钟 − 启动≈200ms）= **34.6 / 23.9 / 29.9 MB/s**，
落在（run3 略低于窗口最小值约 12%，因末窗口只覆盖排空最密集段）各次窗口速率区间内。

**启动打点（5 次中位）**：`renderer_ready_ms` **184**、`pre_first_submit_ms` **193**、
`first_frame_ms` **206**（区间 174–200 / 182–208 / 196–223）。

## 4. t4 反色缺陷（CPU 与 GPU 同改）

**修复前实测**（绝对像素测试，非 CPU↔GPU 相对对比）：反色格白像素 **0/128**、
原前景色像素 **0**——即"黑底黑字"，与 4B 报告一致。

**根因（两处）**：
1. `CpuRenderer::blit` 的混合底用填充背景色而非 inverse 交换后的 `bgc`；
2. **填充色本身**也用原背景色，字形矩形之外的区域仍是旧底色。

**修复**：新建 `crates/render/src/decide.rs` 作为**唯一决策来源**：
`CellDecision { bg, fg, blend_bg, line, span, is_tail }` + `decide(cell, &DecideCtx)`。
CPU 的 `draw_row` 与 GPU 的 `build_instances` 都改用它（不再各自分叉）；
`bg`（填充）与 `blend_bg`（字形混合底）都取 inverse/选区交换后的底色。
`decide::rgb_to_u32` 补回 `0xFF00_0000` alpha 前缀（新建该模块时曾漏，会让所有底色
变成 `0x00RRGGBB`），保证非反色格逐字节不变。

**修复后**：CPU 白 **74** / 黑 9；GPU 白 **74** / 黑 9；彩色反色为原 fg 色 74 / 原 bg 色 9。

**门禁（绝对像素，可证伪）**：`crates/render/tests/inverse_pixels.rs` 5 用例
（默认色 inverse、彩色 inverse、选区+inverse、光标+inverse、GPU 侧同断言）。
**证伪已验证**：把 `decide` 的 `bg` 回退成原背景 → 3 个用例红；恢复后 5 过。

**e2e**：case34（cpu 轮）+ GPU 轮同名用例，`printf ESC[7m` 输出 10 格反色空格后
截图统计白色像素，阈值 **≥768**（理论 10×8×16 = 1280 的 60%，留字体/网格差异余量），
实测 cpu 与 gpu 均 **2742**。

**期望值变化（仅 inverse 用例）**：`render_pixels::render_selection_inverts_selected_cell`
原断言"A 格（未选、反显）背景仍为原背景蓝"**正是旧缺陷行为**，改为"底色=原前景色（红）、
字形=原背景色（蓝）"。`gpu_parity` 8 用例未改阈值、全过。

## 5. 验收

- `cargo test --workspace`：**129 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**90/90 PASS**，`GPU_SKIP=0`，GPU 轮 **5/5**
- Windows 目标 `cargo tree`：`wgpu`/`pollster` 计数 **0**（每个 tag 前均复核）

## 6. 未解决问题

1. **最小化守卫的 e2e 无法证伪**（已验证事实）：X11 的 `windowminimize` 不产生
   `Resized`，case33 恒绿；守卫正确性由单测承担。Windows 侧需真实复验（3E）。
2. **PTY 阻塞写**（已验证事实，遗留）：主端 writer 为阻塞模式、缓冲约 64KB，
   大文本排空可能阻塞主线程；`PASTE_CHUNK` 降到 4KB 会严重拖慢排空（已回退）。
   彻底解决需 `fcntl(O_NONBLOCK)`（新增 `libc` 直接依赖）或独立写线程（需改 `Pty` trait）。
3. **`render_target` 判据依赖字体度量**（工程判断，置信度中）：若某字体 cell 极小
   （理论下限 1×1，实际 ≥6），"不足一格"判据会退化；届时需改为显式最小网格常量。
4. **反色语义变更需视觉确认**：反色格从"黑底黑字"变为"原前景底 + 原背景字"，
   与原终端语义一致，但用户若有依赖旧观感的配置（如自定义 palette 下反色对比度），
   需在 office-pc 上目视确认。

## 7. Windows 3E 交接

1. **最小化/恢复真实复验**：Windows 上最小化发 1×1（有时 0×0），本轮守卫在单测层面
   已验证；需在 Windows 会话确认：最小化 → 恢复后网格仍为原行列、终端内容完整、
   无 `vt resize` 日志。
2. **依赖树与构建**：`cargo tree --target x86_64-pc-windows-gnu -e normal` 应继续无
   `wgpu`/`pollster`；`cargo build --target x86_64-pc-windows-gnu` 应零告警（本轮新增
   的 `decide.rs` 无平台分支，GPU 路径仍在 `cfg(target_os = "linux")` 内）。
3. **反色观感**：Windows 终端同样受 t4 影响（`ESC[7m` 现在显示为原前景底 + 原背景字），
   如有依赖旧行为的截图基线需更新。
4. **观测新字段**：`feed_bytes_per_s` / `chan_depth_max` / `renderer_ready_ms` /
   `pre_first_submit_ms` 在 Windows 同样可用，可用于 3E 的启动与吞吐取数。

## 8. 手动清单（≤3）

1. office-pc：`ESC[7m` 反色文本是否显示为"亮底暗字"（而非全黑），以及自定义 palette
   下反色的对比度是否可接受。
2. office-pc / Windows：最小化再恢复后，终端内容与行列数是否完整（本机 :99 无法证伪）。
3. 12px 滚动条与 paste 后的即时刷新在真实会话中的手感确认（沿用 4B-fix2/fix3 的手动项）。
