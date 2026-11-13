# PHASE4B-FIX3-LINUX：粘贴后不刷新的取证与加固 / 滚动条 12px

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 状态摘要

| 项 | 结果 |
|---|---|
| 任务 A 复现 | **在 :99 上未能复现**用户现象（短文本粘贴 500ms 内 dump 即含文本） |
| 任务 A 根因 | 代码审查确认假设 **(c)**：`do_paste` 同步路径写完 PTY 后不请求重绘 |
| 任务 A 修复 | 已做最小加固（粘贴后 `urgent_render` + `request_redraw`） |
| 任务 A 门禁 | **无法构造可证伪用例**（原因见 §3），故 **tag `phase4b-fix3-a` 未打** |
| 任务 B（滚动条 12px） | 已完成并通过全部验收，**tag `phase4b-fix3-b`** |
| 交付 tag | `phase4b-fix3` **未打**（依赖 A 的验收结论） |

## 1. 复现尝试与证据

e2e 环境：`:99` + Xvfb + openbox，debug 构建，`APP_DUMP_SNAPSHOT` 文本钩子 + 截图。

| 场景 | 结果 |
|---|---|
| 短文本（`PASTED_ABC`）+ 拖选后右键粘贴（CLIPBOARD） | **未复现**：粘贴后 500ms dump 已含文本，2000ms 仍含 |
| 大文本 1.5MB（走 `pending_paste` 预算排空） | 3s 内 dump 无内容——但该场景内容被 shell 当作命令逐行执行、回显爆炸，不具代表性 |
| `stty -echo`（关闭回显）+ 右键粘贴 | 修复前后 dump mtime 均变化，用例恒绿 |

**版本二分未进行**，理由：用例在 HEAD 上即为绿（未复现），二分前提"先确认用例在 HEAD 上确实为红"不成立。

一次方法学失误需要记录：最初两轮复现脚本**漏了 `backend=cpu`**，默认走 GPU 后端，而 GPU 后端不回读、`APP_DUMP_SNAPSHOT` 不写 → 误判为"粘贴无回显"。固定 `[renderer] backend = "cpu"` 后短文本场景即正常。

## 2. 根因（代码审查，假设 (c)）

`crates/app/src/lib.rs::do_paste`：

```rust
// 修复前
} else if let Some(pty) = self.pty.as_mut() {
    write_pty(pty, norm.as_bytes());     // 同步写入 PTY，然后直接返回
}
```

- `< 1MB` 走这条同步路径；`>= 1MB` 走 `pending_paste`，由 `about_to_wait` 每轮预算排空。
- 两条路径写完后都**没有** `urgent_render` / `request_redraw`（排空路径只有 `needs_more` 时的 `request_redraw`，且不置 urgent）。
- 因此画面刷新**完全依赖** PTY 回显字节到达 reader → channel → wake → `about_to_wait` 的帧调度。若回显缺失（shell 关 echo、readline 尚未处理、粘贴内容被 bracketed paste 包裹后由应用延迟回显）或回显到达但帧间隔未到期，画面就会停在旧内容，直到下一个事件（回车、键入）驱动。
- 这与用户描述"内容已实际送入 shell、回车后执行结果正确，但粘贴当下不回显，直到回车或再键入才刷新"一致。

**三个假设的排查结论**：

- (a) 唤醒/调度丢失：**未观测到**——短文本场景 dump 在 500ms 内有内容，说明 wake → feed → 渲染链路正常。
- (b) 脏行未标记：**未观测到**——dump 内容正确（vt 已更新），且渲染输出与 vt 一致。
- (c) 粘贴路径未置 `urgent_render`：**成立**（代码事实）。

## 3. 门禁缺口（为什么没有 tag -a）

要求"新增的 e2e 用例须由红转绿"。实际尝试全部失败：

1. **鼠标右键粘贴**本身产生 `PointerButton` 事件 → 从 4B-fix1 起 `RedrawRequested` 被处理 → 无论是否加固都会刷新，信号被掩盖。
2. **`stty -echo` 关闭回显**后仍会在下一个帧调度周期刷新（`WaitUntil` ≤ 100ms），350ms 窗口内观察不到差别。
3. **键盘粘贴 Ctrl+Shift+V**：`xdotool key --window` 走 `XSendEvent`，组合键被应用忽略（回车后 dump 无粘贴文本 → 粘贴未发生），无法自动化；不带 `--window` 需要抢走真实焦点，在 :99 的 openbox 下不可靠。

结论：**当前 e2e 设施无法分离"粘贴主动排帧"与其他重绘触发**。因此该加固虽有代码层依据，**没有可证伪的回归门禁**，我不把它标为"已修复"。

**推翻条件**：若在 office-pc 实测仍不回显，说明 (c) 不是唯一原因（例如还有 X11 焦点/事件路由问题）；若实测已修复，则该加固有效，可补一个基于 `APP_PERF_FRAMES` 帧计数的行为断言（粘贴后 50ms 内必须出现新帧）作为门禁。

## 4. 改动清单

### 4.1 粘贴加固（提交 784e322，未单独 tag）

- `do_paste` 同步路径写完 PTY 后：`self.urgent_render = true` + `window.request_redraw()`。
- `pending_paste` 每轮排空后：在原有 `request_redraw()` 基础上补 `urgent_render = true`（排空期间内容在变，下一帧应尽快到来）。

**没有**采用"粘贴后强制整帧重绘"——只请求尽快排一帧，脏行仍由 vt 的 `dirty_rows` 决定，符合"不得用整帧重绘掩盖根因"。

### 4.2 回退的尝试（PASTE_CHUNK）

曾把 `PASTE_CHUNK` 由 64KB 降到 4KB（想让单次 `write` 小于 PTY 缓冲 64KB，避免阻塞主线程），但：

- 排空吞吐下降 16 倍，e2e case25（10MB 键入）在超时内只送达 **4.39MB**（确定回归）；
- 大文本阻塞写是否真实影响用户场景未证实。

已回退为 64KB，阻塞写问题登记为未解决项（§6）。

### 4.3 滚动条 12px（tag phase4b-fix3-b）

`render::SCROLLBAR_WIDTH` 8 → **12**（4 → 8 是 4B-fix2）。四处引用同一常量，无新增配置：

| 位置 | 说明 |
|---|---|
| `render::CpuRenderer::draw_scrollbar` | 轨道 + 拇指绘制 |
| `render::gpu::GpuRenderer::build_instances` | 轨道 + 拇指 quad |
| `capture_band` / `capture_band_char_row` / `restore_band` | 右缘条带备份 |
| `app::hit_scrollbar` | 命中测试 |

核对：`cargo test` 的增量对拍（含滚动条 4 组场景）全过；e2e case18（拖拽到顶）通过；case29（网格外背景色）取样点在左下角，位于 12px 条带之外，仍 9/9 通过。**未放宽任何阈值**。

## 5. 验收

- `cargo test --workspace`：**119 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**87/87 PASS**，`GPU_SKIP=0`，GPU 轮 4/4

## 6. 未解决问题

1. **粘贴不回显未能在自动化环境复现**（置信度中）：根因判定为 (c) 且已加固，但缺少可证伪门禁，需 office-pc 实测确认。推翻条件见 §3。
2. **`pending_paste` 的阻塞写**（已验证事实）：PTY 主端 writer 是阻塞模式、缓冲约 64KB，`drain_pending` 用 `write` 而非 `write_all` 时仍可能阻塞在缓冲满；把块降到 4KB 会严重拖慢排空（已回退）。彻底解决需要把 writer 设为非阻塞（`fcntl(O_NONBLOCK)`，需引入 `libc` 直接依赖）或改由独立写线程（需 `Pty` trait 变更，本轮禁止）。**未修**。
3. **Shift+Insert 无默认绑定**（已验证事实）：`default_key_bindings()` 只有 Ctrl+Shift+C/V 与鼠标 Middle=Copy、Right=Paste。用户若按 Shift+Insert 实际不会粘贴；若需要该键，应加默认绑定（属新功能，未擅自添加）。
4. **PRIMARY 中键**默认是 **Copy** 而非 Paste（`PasteSelection` 需在配置里显式绑定）。用户描述的"PRIMARY 中键粘贴"在当前默认配置下不成立，需确认其配置。

## 7. 手动清单（≤3）

1. office-pc 上用当前构建（已含粘贴后主动排帧）复测：`stty -echo` 后右键粘贴一段短文本，观察是否在粘贴当下立即出现（不依赖回车/键入）。若仍不回显，请告知，说明 (c) 不是唯一原因。
2. 确认你的粘贴键：默认只有右键 / Ctrl+Shift+V；若习惯 Shift+Insert 或中键 `PasteSelection`，需要在配置里绑定（我可以加默认绑定，需你确认）。
3. 确认 12px 滚动条点选手感；若觉得遮挡右侧内容过多，可改为"滚动时才显示"（需新增配置项）。
