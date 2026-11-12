# PHASE3C-LINUX：合并 Windows / release 零告警 / 自适应帧间隔 / present 核查 / 复用处置 / 启动分段

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。
本轮每个 tag 之前均跑通：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo clippy --release --workspace --all-targets -- -D warnings`、`cargo build --release`（零告警）、`scripts/e2e/run_all.sh`，并把结果写入该提交信息。

## 1. 合并 ali/win（任务 1）

- **实际 win hash = `4bd466c`**（`git ls-remote ali` 核对；本轮开始时为 41dc491，任务书预期的 4bd466c 即当时最新）。
- merge-base = `f32b986`（phase3a-done 交付提交）。
- win 侧相对 merge-base 的改动**仅**：`vt-verify-win/`、`scripts/windows/`、`docs/WIN-NOTES.md`、`PHASE2D-WIN.md`、`PHASE3A-WIN.md`，以及 `.gitignore` 新增 `/vt-verify-win/conpty-test/`。**无 shared 代码改动** → 符合预期。
- `git merge --no-ff` 无冲突（合并提交 `b7f6ee6`）；合并后 shared 代码零改动，Linux 全量验收通过。

## 2. release 告警清零（任务 2）

| 位置 | 告警 | 修复 |
|---|---|---|
| `render/src/lib.rs` `drawn_rows` | release 下 assigned but never used（2 条） | `#[cfg(debug_assertions)]` 门控声明与累加（仅 debug 的 [incr] 统计使用） |
| `app/src/lib.rs` `t_present` | release 下 unused variable | `#[cfg(debug_assertions)]` 门控（仅 debug 的 APP_PRESENT_STATS 使用）；任务 3 后该变量改为 release 也用于 APP_PERF_FRAMES |

并把这些变成门禁：`run_all.sh` 开头执行 `cargo build --release -p app`（计数 warning/error）与 `cargo clippy --release --workspace --all-targets -- -D warnings`，任一非零即中止，通过时输出 `PASS release-zero-warning`（计入用例）。

## 3. APP_PERF_FRAMES 帧耗时日志（任务 3）

- `APP_PERF_FRAMES=1`（**release 与 debug 均可用，默认关闭**），每 50 帧输出：
  `[perf] frames n=50 render_ms(med/p95) present_ms(med/p95) coalesced=<被帧间隔合并的唤醒数> interval_ms=<与上一帧实际间隔> ~<fps>`。
- 定位：**性能诊断日志（非测试钩子）**，release 保留；未开启时仅一次 Option 判空。
- 样例（release，120x30@1x，逐行输出）：render 5–6ms（p95 6–12）、present 2ms（p95 3–7）、interval 11–12ms（≈83–90fps）。

## 4. 自适应帧间隔（任务 4）

参数（常量登记于 `crates/app/src/framepace.rs`）：`frame_interval = clamp(PACE_K × EMA(渲染耗时), 8ms, 100ms)`，`PACE_K=3.0`、`PACE_EMA_ALPHA=0.3`、下限 8ms、上限 100ms。纯函数 `target_interval`/`next_ema` 配 3 个单测（边界/NaN/收敛/序列响应）。

- 输出路径按 frame_interval 等待；键盘/鼠标/滚轮/IME 走 urgent 路径不受自适应间隔拖延，但仍受 8ms 下限（避免洪峰中输入帧背靠背）。
- **尾沿保证**：未到期时 `ControlFlow::WaitUntil(上次渲染 + 目标间隔)`，输出停止后最后一帧必被渲染（e2e case28：20 万行输出后 1s 内 dump 出现结束标记）。
- 顺带修复：`run()` 在 `run_app` 返回后先 `handler.rx.take()`（drop 接收端），避免 channel 积压时读线程阻塞在 send 导致 reader join 等满 5s。

**吞吐（release，Xvfb :99，2,000,000 行满屏滚动，各 2 次）**

| 配置 | 3B（固定 8ms） | 3C（自适应） | 变化 |
|---|---|---|---|
| 10x3（基线） | 1449 / 1636ms | 1270 / 1390ms | — |
| 120x30 | 6034 / 6932ms | 4736ms | 改善 21–32% |
| 200x60 | 29147 / 34690ms | 6479ms | 改善 78–81% |

**渲染开销占比（对 10x3 基线）：120x30 = 71.9%、200x60 = 79.5%，仍高于 ≤30% 目标（未放宽）。** 构成（APP_PERF_FRAMES + 总时长推算）：

| 项 | 120x30 | 200x60 |
|---|---|---|
| render | ≈32%（6/19ms） | ≈30%（21/70ms） |
| present | ≈11%（2/19ms） | ≈16%（11/70ms） |
| feed（libghostty 解析） | ≈27% | ≈20% |
| WaitUntil 主动等待 | ≈30% | ≈34% |

洪峰帧间隔收敛值：120x30 **19–20ms（≈50fps）**、200x60 **70ms（≈14fps）**；200x60 约 14fps 属任务预期区间（10–15fps），在 CPU 软提交路径下可接受（同场景总时长已从 29–35s 降到 6.5s）。

**键入回显延迟**（debug + dump，20 次中位）：自适应 25.36ms vs 关闭节流 25.50ms；3B 为 25.61ms → 未增加（远小于 5ms 上限）。

## 5. present 瓶颈核查（任务 5）

- `xdpyinfo`（:99）显示 23 个扩展中**含 MIT-SHM**。
- softbuffer 0.4.8 X11 后端（`src/backends/x11.rs`，只读查阅）：
  - `enum Buffer { Shm(ShmBuffer), Wire(util::PixelBuffer) }`（行 161–167）；
  - 创建时 `if display.is_shm_available { Buffer::Shm(..) } else { Buffer::Wire(..) }`（行 284–292）；
  - `is_shm_available`（行 868 起）创建 4KB shm 段并 attach/detach 探测，失败即 false；
  - `present_with_damage`（行 425）：`Buffer::Wire` → 打印 `Falling back to non-SHM method`（行 437）并 `put_image`（行 439–441，走协议）；`Buffer::Shm` → `shm_put_image`（行 458–480）；
  - `present()` = 全窗口单 damage 的 `present_with_damage`（行 515–520）。
- `APP_PRESENT_FULL` 提升为 **release 诊断开关**（与 APP_PERF_FRAMES 同类，默认关闭、不改变默认路径）；debug 专用 APP_PRESENT_STATS 保留。
- **release 实测**（Xvfb :99，空闲 + 逐行 ticker）：120x30 脏区 present 中位 2–4ms（p95 4–6）、全缓冲 2ms（p95 3–5）；200x60 全缓冲 10ms。洪峰下 present 占一帧 **10.5%（120x30）/ 16%（200x60）**。
- **结论修正**：3B 的"present 76–84ms/帧"来自 **debug 构建 + 200x60@2x（27MB 帧）**，不代表 release；release 下 present 仅占 10–16%，**不是主因**；主因是 feed（20–27%）+ render（30–32%）+ 主动等待（30%）。debug 的 present 数字不再引用。

## 6. 滚动位移复用处置（任务 6）

- **限时诊断**：新增 debug 钩子 `APP_FRAME_HASH=<path>`（逐帧记录全帧/逐行像素 FNV 哈希）。用 case26 序列对拍"复用开 vs 强制全量"：首个不一致出现在第 1–2 帧，差异覆盖顶部 9–20 行。
- **根因（已定位）**：`find_scroll_shift` 原实现统计"匹配行**总和**"，而屏幕上大量空行彼此相同 → 任意偏移都能匹配过半 → 空屏/稀疏屏被误判为滚动。
- **修复**：改为"最长**连续**匹配段 + 段内非空行数 ≥ max(2, rows/8)"。修复后 case26 序列**最终截图 AE=0**，滚动专项单测与 3000 步随机序列通过。
- **收益评估（决定性）**：2M 行滚动（release）——120x30 复用关 3979ms / 开 4623ms（**慢 16%**）；200x60 关 6004ms / 开 6359ms（**慢 6%**）。远未达"≥25%"门槛。
- **处置：按任务要求删除实现**（`row_fingerprint`/`find_scroll_shift`/`blit_scroll`/`scroll_reuse` 及 app 侧 `APP_SCROLL_REUSE`）。删除原因：自适应帧间隔已把渲染总时长压到少数帧，而复用的固定成本（逐 cell 指纹哈希 + 整屏 memmove 2.2MB/27MB）超过省下的行重绘。保留：行级增量渲染、滚动条条带备份、滚动序列增量 vs 全量对拍单测（`scroll_sequence_matches_full`）、e2e case27（去掉开关）、以及 `APP_FRAME_HASH` 诊断钩子。

## 7. 启动补充分段（任务 7）

新增 `[perf] main_ms`（进程启动→main 首行，覆盖 loader/动态链接；由 `/proc/uptime` 与 `/proc/self/stat` 的 starttime 按 CLK_TCK=100 推算）、`config_ms`、`pty_spawn_ms`、`first_pty_byte_ms`。

| 分段（release，5 次） | 无配置 | examples/linux.toml |
|---|---|---|
| main_ms（loader→main） | 0–10ms | 0–10ms |
| config_ms | 0.1ms | 0.1ms |
| fontdb_ms | 86.1–98.8ms | 83.7–98.2ms |
| 主字体解析（font_ms=0） | 28.0–29.1ms | （Consolas 缺失→Monospace，同类量级） |
| window_ms | 0.6–2.4ms | 0.6–2.4ms |
| surface_ms | 0.5–1.7ms | 0.5–1.7ms |
| pty_spawn_ms | 3.2–4.9ms | 2.7–3.9ms |
| first_pty_byte_ms = first_frame_ms | 215–235ms | 158–189ms |
| bg_fonts_ms（CJK 到位） | 399–406ms | 639–694ms |

结论（工程判断）：Linux 首帧 ≈220ms ≈ loader 10 + fontdb 90 + 主字体解析 29 + 窗口/表面 3 + PTY spawn 4 + 等 shell 首个输出 20–80ms。**首帧与首个 PTY 字节同时刻**说明首帧瓶颈是"等 shell 输出"而非渲染。Windows 报告的 819ms 中约 470ms 未解释，应重点用 main_ms（loader/杀软）、pty_spawn_ms 与 first_pty_byte_ms（ConPTY 启动与 shell 首输出）定位。

## 8. 未解决问题

1. **渲染开销占比仍未达 ≤30%**（71.9%/79.5%，对 10x3 基线）。构成见 §4：feed 20–27%、render 30–32%、主动等待 30%、present 10–16%。要显著改善需降低 feed 成本（libghostty 解析，非本仓库可调）或改变渲染架构（GPU），属阶段 3 决策范围。
2. **debug 构建下 libghostty 页面校验成本极高**（PageList.grow/verifyIntegrity，3B gdb 已证），任何性能结论必须用 release；e2e 大量输出用例（case28）因此不做退出断言。
3. fontdb 系统字体扫描 ≈90ms、主字体解析 ≈29ms 仍占首帧大头；可考虑扫描结果缓存/并行化（未实施，避免新增依赖与复杂度）。
4. Windows 首帧 819ms 的未解释部分待 Windows 侧用本轮日志取数。

## 9. 给 Windows 侧的交接

1. **需复验**：合并后 shared 代码零改动，但建议复跑增量渲染一致性（e2e 等价用例）与 `present_with_damage` 在 Win32 后端的行为。
2. **需取数**（debug 专有开关，release 不含）：`APP_PERF_FRAMES=1`、`APP_PRESENT_FULL=1`（release 也可用）、`APP_NO_RENDER=1`、`APP_NO_THROTTLE=1`、`APP_FORCE_FULL_REDRAW=1`、`APP_FRAME_HASH=<path>`（debug 专有）、`APP_RANDOM_SEED`。
3. **启动分段取数**：用 `[perf] main_ms / config_ms / fontdb_ms / font_ms / window_ms / surface_ms / pty_spawn_ms / first_pty_byte_ms / first_frame_ms / bg_fonts_ms` 定位 819ms 的构成，重点看 main_ms（loader/杀软）与 first_pty_byte（ConPTY + shell 首输出）。
4. **ConPTY 背压**：有界 channel（256）在 reader 侧已处理"满时先 wake 再阻塞 send"；建议 Windows 复跑 1MB/10MB 粘贴与 2M 行输出，确认无死锁；另注意 Window 上 20 万行输出的 debug 构建同样受 libghostty 校验影响。
5. Windows 专属文件本轮未编辑；`ali/win` 实际 hash 以 `git ls-remote ali` 为准（本轮合并的是 4bd466c）。

## 10. 手动清单（≤3）

1. Windows 侧按 §9 取数，定位首帧 819ms 中未解释的约 470ms。
2. 真实 GPU X11（非 Xvfb / 非软渲染）下复测 present 与帧间隔（本轮所有数字均来自 Xvfb 软提交）。
3. 人眼确认 200x60 洪峰下 ≈14fps 与 120x30 ≈50fps 的观感是否可接受（自适应间隔是否引起可感知的"批量刷新"）。

## 11. 验收

- `cargo test --workspace`：**98 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**82/82 PASS，连续 3 次全过**
