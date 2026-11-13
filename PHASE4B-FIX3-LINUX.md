# PHASE4B-FIX3-LINUX：粘贴后不刷新的取证与加固 / 滚动条 12px

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 状态摘要

| 项 | 结果 |
|---|---|
| 任务 A 复现 | **在 :99 上未能复现**用户现象（短文本粘贴 500ms 内 dump 即含文本） |
| 任务 A 根因 | 代码审查确认假设 **(c)**：`do_paste` 同步路径写完 PTY 后不请求重绘 |
| 任务 A 修复 | 已做最小加固（粘贴后 `urgent_render` + `request_redraw`） |
| 任务 A 门禁 | **已补齐并可证伪**（debug 钩子 `APP_TEST_PASTE` + e2e case32），**tag `phase4b-fix3-a`** |
| 任务 B（滚动条 12px） | 已完成并通过全部验收，**tag `phase4b-fix3-b`** |
| 交付 tag | `phase4b-fix3` ✅ |

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

## 3. 门禁（case32，可证伪）

**为什么需要 debug 钩子**：鼠标/键盘第三方触发粘贴都会顺带产生 `RedrawRequested`（4B-fix1 起处理），或 `xdotool key --window` 的 XSendEvent 被应用忽略——无法把"粘贴主动排帧"与其他重绘触发分离，用例恒绿。

**方案**：app 内新增 debug 专有钩子 `APP_TEST_PASTE=<文本>` + `APP_TEST_PASTE_AT_MS=<延迟>`，由 `about_to_wait` 在指定延迟后调用 **与正式路径同一个** `paste_text()`；正式路径 `do_paste` 也改为调用 `paste_text`，保证门禁验证的就是生产代码（release 不含该钩子）。

**e2e case32 流程**：`backend=cpu` + `APP_FRAME_HASH` → `stty -echo` 关闭回显 → 等画面静止（此后无任何回显字节驱动渲染）→ 钩子在 3.5s 时触发粘贴 → 断言出现 `[perf] test_paste_first_frame ... urgent=true` 且帧哈希行数增加。

**证伪证据（实测）**：

| 状态 | frames | 指标行 | 结果 |
|---|---|---|---|
| 回退"写完主动排一帧" | 4 → **4** | **无** | 红 ✅ |
| 恢复加固 | 4 → **5** | `latency_ms=105 urgent=true` | 绿 ✅ |

**关于 50ms**："粘贴后 50ms 内出帧"在 **debug 构建下达不到**——实测首帧完成 104–105ms，主因是 debug 渲染本身 20–30ms 加每帧写盘（`APP_FRAME_HASH` + dump）。因此 case32 以 **`urgent=true` 为主判据**（这正是加固的本质：该帧由粘贴主动请求，而不是等下一个调度周期），latency 只作日志记录，未把阈值放宽后声称通过。

**推翻条件**：若 office-pc 实测仍不回显，说明 (c) 不是唯一原因（需查 X11 焦点/事件路由）。

## 4. 改动清单

### 4.1 粘贴加固（784e322 起，b7aad4e 收口补门禁；tag phase4b-fix3-a）

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
- `scripts/e2e/run_all.sh`：**88/88 PASS**（含 case32），`GPU_SKIP=0`，GPU 轮 4/4

## 6. 未解决问题

1. **粘贴不回显未能在自动化环境复现**（置信度中）：根因判定为 (c)，已加固并有可证伪门禁（case32，回退即红），但仍需 office-pc 实测确认修复对用户场景有效。推翻条件见 §3。
2. **`pending_paste` 的阻塞写**（已验证事实）：PTY 主端 writer 是阻塞模式、缓冲约 64KB，`drain_pending` 用 `write` 而非 `write_all` 时仍可能阻塞在缓冲满；把块降到 4KB 会严重拖慢排空（已回退）。彻底解决需要把 writer 设为非阻塞（`fcntl(O_NONBLOCK)`，需引入 `libc` 直接依赖）或改由独立写线程（需 `Pty` trait 变更，本轮禁止）。**未修**。
3. **Shift+Insert 无默认绑定**（已验证事实）：`default_key_bindings()` 只有 Ctrl+Shift+C/V 与鼠标 Middle=Copy、Right=Paste。用户若按 Shift+Insert 实际不会粘贴；若需要该键，应加默认绑定（属新功能，未擅自添加）。
4. **PRIMARY 中键**默认是 **Copy** 而非 Paste（`PasteSelection` 需在配置里显式绑定）。用户描述的"PRIMARY 中键粘贴"在当前默认配置下不成立，需确认其配置。

## 7. 手动清单（≤3）

1. office-pc 上用当前构建（已含粘贴后主动排帧）复测：`stty -echo` 后右键粘贴一段短文本，观察是否在粘贴当下立即出现（不依赖回车/键入）。若仍不回显，请告知，说明 (c) 不是唯一原因。
2. 确认你的粘贴键：默认只有右键 / Ctrl+Shift+V；若习惯 Shift+Insert 或中键 `PasteSelection`，需要在配置里绑定（我可以加默认绑定，需你确认）。
3. 确认 12px 滚动条点选手感；若觉得遮挡右侧内容过多，可改为"滚动时才显示"（需新增配置项）。
