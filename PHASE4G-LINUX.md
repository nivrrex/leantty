# PHASE4G-LINUX：视口上滚隐藏光标 / PTY 写停顿量化

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 视口上滚后隐藏光标 | **已修**（vt 层改用 `cursor_viewport`）；tag `phase4g-t1` |
| t2 PTY 写不阻塞主线程 | **量化后判定无需实施**（最大停顿 90–95ms < 100ms，且与数据量无关）；**未打 t2 tag** |

## 1. t1 取证（4G-t1a）

| 项 | 结论 |
|---|---|
| libghostty 接口 | `RenderState::cursor_viewport() -> Result<Option<CursorViewport>>`（`vendor/libghostty-rs/crates/libghostty-vt/src/render.rs:970`）：`None` = 光标不在视口；`CursorViewport{x,y,at_wide_tail}` 为视口内坐标。文档示例（同文件 442–446）即按此判定 |
| 另一入口 | `Snapshot::cursor()` 返回 `Cursor{ viewport: Option<CursorViewport>, visible, blinking, … }`（同文件 1917–1930），同样以 `viewport` 为 `None` 表示不在视口 |
| `crates/vt` 现状（修复前） | `snapshot_impl` 用 `Terminal::cursor_x()` / `cursor_y()`（**活动区**坐标）+ `Snapshot::cursor_visible()`（仅 DECTCEM 模式位），**完全没有查视口** → 视口上滚到 scrollback 后光标仍被画在原屏幕位置 |
| 复现证据 | 80×10 终端输出 30 行后上滚 5 行：`cursor.visible=true`、位置 `(8,9)`（活动区坐标） |

**根因**：快照把"终端模式允许显示"误当成"应当显示"，缺少"是否在视口内"这一维。

## 2. t1 修复

`crates/vt/src/terminal.rs::snapshot_impl`：

```rust
cursor: {
    let style = map_cursor_style(snap.cursor_visual_style()?);
    match (snap.cursor_visible()?, snap.cursor_viewport()?) {
        (true, Some(v)) => Cursor { x: v.x, y: v.y, visible: true, style },
        _ => Cursor { x: 活动区x, y: 活动区y, visible: false, style }, // 坐标仅作诊断
    }
}
```

- 视口内坐标随视口偏移实时换算（上滚 N 行时可见光标的 y 增加 N）。
- 备用屏无 scrollback 偏移 → 行为不变。
- `ESC[?25l`（模式隐藏）优先，仍为不可见。
- **未在 renderer 里加"scrolled 就不画"的补丁**：接口存在，问题在 vt 层。

**闪烁定时器（t1d）**：app 中**不存在**光标闪烁逻辑（静态块光标），因此"光标不可见时因闪烁唤醒重绘"的问题不存在，无需改动；`APP_PERF_FRAMES` 下上滚静止期间无额外帧。

## 3. t1 测试与证伪

| 层 | 用例 | 结果 |
|---|---|---|
| vt（`tests/cursor_viewport.rs`） | 上滚隐藏 / 回底恢复且位置正确；可见时坐标随视口偏移；备用屏；`ESC[?25l` 优先 | 4 passed |
| render（`tests/cursor_viewport_pixels.rs`） | CPU 与 GPU：光标色取品红，回底光标像素 **128**、上滚 **0**、回底后恢复 128 | 2 passed（GPU 无适配器时 SKIP） |
| e2e `case37`（cpu 轮 + GPU 轮） | 输出 200 行 → 滚轮上滚 8 格 → 3 次截图（间隔 0.5s）光标色像素应全为 0 → 滚回底部应 >0 | 实测：回底 **273**、上滚 **0/3 全零**、回底后 **273** |

**证伪（已验证）**：

| 状态 | vt 单测 | e2e case37 |
|---|---|---|
| 回退修复（用活动区坐标） | **2 failed** | 上滚采样 **0/3 全零**（即光标仍可见）→ 红 ✅ |
| 恢复修复 | 4 passed | **3/3 全零** → 绿 ✅ |

**顺带修正 e2e 取样方法（已验证事实）**：`convert -fuzz -fill white -opaque <目标>` 后接
`+opaque white` 会把图中**原本的白色元素**一并计入（首版 case37 因此把白字当成光标色：
回底 998、上滚仍非零）。正确顺序是**先把非目标色变黑**（`-fill black +opaque <目标>`）
再把目标色变白，然后统计白色。已同步修正 case36/case37 与 GPU 轮的取样函数；
修正后 case36 仍为 2734/2751、case37 回底 273 → 上滚 0。

## 4. t2a 停顿量化（结论：无需实施）

**观测**：新增 `loop_gap_max_ms`（`APP_PERF_FRAMES` 门控、release 可用、默认关闭仅一次
`Option` 判空），在 `about_to_wait` 首部统计两次迭代间隔的最大值，并**每 5 秒独立打印**
`[perf] loop_gap_ms=N`（不依赖 50 帧窗口，静止画面也能观测）。

**测量场景**：`/bin/sh` 前台运行 `sleep 20`（不读 stdin），`APP_TEST_PASTE=@<file>` 在
启动后 3s 触发粘贴（走 `paste_from_text` 生产路径）。

> `APP_TEST_PASTE` 新增 `@<file>` 形式：环境变量单值上限 `MAX_ARG_STRLEN`≈128KB，
> 无法传 1MB 量级素材（debug 钩子，release 不含）。

**结果**：

| 粘贴量 | 最大 loop gap |
|---|---|
| 780KB（3 次） | 129 / **95** / 95 ms（中位 95） |
| 5.07MB（1 次） | **90** ms |

- **停顿与数据量无关**（5MB 反而更小）→ 不是 PTY 写阻塞；129ms 那次应归因启动期渲染。
- 按任务判据（最大停顿 <100ms 则停止 t2）→ **不实施写线程，不打 t2 tag**。

**与 4B-fix3 假设的差异**（待验证假设，置信度中低）：4B-fix3 记录"前台不读 stdin 时
64KB 以上粘贴会阻塞主线程"，本机**未复现**——780KB 与 5MB 同量级，说明 Linux PTY 的
写缓冲足以吸收该量级数据（或 `sleep` 期间 tty 层仍在排空）。**推翻条件**：若真实办公机
上用 ≥50MB 粘贴、或前台进程长期占满 tty 输入队列时出现 ≥100ms 停顿，则本结论不成立，
应按授权方案（`Pty` trait 新增带默认实现的 `take_writer`）实施写线程。

**保留的观测**：`loop_gap_ms`（主线程停顿的通用指标）与 `APP_TEST_PASTE=@file`
（解除 env 长度限制）都保留，属诊断范畴，不改变生产路径。

## 5. 未解决问题

1. **t2 未实施**（已验证事实）：本机测不出 ≥100ms 的写阻塞。若在真实办公机复现，需按
   任务 t2b 的契约实施（有界字节队列、Ctrl-C 丢弃未写数据、join 超时、EIO/EPIPE 不 panic）。
2. **`loop_gap_ms` 的归因粒度**（工程判断，置信度中）：它只记录"两次迭代最大间隔"，
   不含阻塞原因。若要区分"write 阻塞"与"渲染慢"，需在 write 前后各打一个时间戳
   （本轮未做，因为 5MB 已证明与数据量无关）。
3. **上滚后光标不可见的边界**（已验证事实）：光标恰好位于视口最后一行且上滚 1 行时，
   libghostty 判定为不可见（视口外），与 xterm 语义一致；若将来要"半行可见"效果需
   overscan 支持（未实现）。

## 6. Windows 3G 交接

1. **t1 修复跨平台**：`cursor_viewport()` 是 libghostty 的通用 render-state 字段，
   无 `cfg` 分支；Windows 上视口上滚同样应隐藏光标。可用
   `cargo test -p vt --test cursor_viewport` 与 `cargo test -p render --test cursor_viewport_pixels`
   复验（4E 已放开 unix 门控）。
2. **t2a 未实施**：Windows 的 ConPTY 写路径未做停顿量化；若 Windows 侧观察到粘贴卡顿，
   应先用等价的 `loop_gap_ms` 观测取数，再决定是否实施写线程（Windows 侧 `Pty` 实现
   不需要改，trait 默认方法返回 `None` 即回退同步写）。
3. **`APP_TEST_PASTE=@file`** 在 Windows 同样可用（debug 钩子），便于以文件提供大块素材。

## 7. 手动清单（≤3）

1. office-pc：滚动到 scrollback 后确认光标消失、滚回底部后恢复（这是本次修复的目标）。
2. 若曾在 office-pc 上遇到"粘贴后界面卡顿数秒"，请记录粘贴量（可用
   `APP_TEST_PASTE=@<文件>` 复现）与 `[perf] loop_gap_ms` 数值——用于判断是否需要实施写线程。
3. 备用屏（vim/less 等）内滚动时光标行为是否正常（本轮用单元测试覆盖，未做真机目视）。
