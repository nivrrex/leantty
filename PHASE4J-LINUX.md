# PHASE4J-LINUX：Windows 3H 合并 / 写线程测试稳定性 / release 停顿门禁 / 字体打点 / 平台限制 / 长跑复测

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3h-win-done-2` | 已合并（tag `phase4j-t1`），共用代码零改动 |
| t2 写线程测试稳定性 | 3 条竞态测试改为确定性设计；两种施压下 400 次全过（tag `phase4j-t2`） |
| t3 release 真实路径停顿门禁 | 新增 case43（cpu+gpu）；**任务原定 <100ms 未达成**，已定位原因并留证据（tag `phase4j-t3`） |
| t4 字体加载内部打点 | 分步之和与组总时长差 0.15–0.17ms（tag `phase4j-t4`） |
| t5 README 平台限制 | 已加四条并标依据（tag `phase4j-t5`） |
| t6 soak 复测 | **未达判据**（CPU +61.2%、GPU +33.8%）；根因未定位 → **不打 t6 tag、不打 phase4j-done** |

## 1. t1 合并证据（ls-remote 实测）

```
68ec3bf28a101ae8a6a4b7d4c3d40d21408ce611	refs/heads/main
73fc6356d17049a174eca4605afd143e006561db	refs/heads/win
0fe3b7f5e88eb0283d9f2991cc55dff6ba5cda8b	refs/tags/phase3h-win-done
2ea9cdd7e196582a351e0f35aa7f8ade4d7af427	refs/tags/phase3h-win-done^{}
d6c46f6aacecb5cbf53517cb4dd162ec2f19698c	refs/tags/phase3h-win-done-2
73fc6356d17049a174eca4605afd143e006561db	refs/tags/phase3h-win-done-2^{}
```

两者都是附注标签：`phase3h-win-done` → `2ea9cdd`（"3H-win: merge phase4h-done, startup breakdown and paste-stall take_writer"），`phase3h-win-done-2` → `73fc635`（"3H-win: record push/tag hashes in the report"），后者是前者后代且等于 win 头 → **取 `73fc635`**。

合并基 `fddc97b`，win 侧 7 个提交、94 文件、+3560/−152。共用/受控范围仅三处：

| 文件 | 说明 |
|---|---|
| `crates/pty/src/windows.rs` | ConPTY 的 `take_writer`（Windows 专属文件，由合并带入） |
| `PHASE3H-WIN.md` | 新增 Windows 报告 |
| `docs/WIN-NOTES.md` | Windows 专属文档更新 |

其余 91 个文件全在 `vt-verify-win/`、`scripts/windows/`。`--no-ff` 合并后：`cargo check --target x86_64-pc-windows-gnu --workspace --tests` 通过、Windows 依赖树 `wgpu`/`pollster` 命中 0、159 测试 0 失败、e2e 98/98（GPU 轮 10/10）。

## 2. t2 写线程相关测试的稳定性

### 2.1 清点（crates/app 内依赖线程时序的测试）

| 测试 | 是否竞态 | 竞态点 |
|---|---|---|
| `writer::tests::paste_queue_respects_cap` | **是** | 断言"持续入队必被拒绝"，前提是生产者一直快于消费者；消费一快就永远看不到拒绝 |
| `writer::tests::ctrl_c_drops_pending_paste_but_keeps_typed` | **是** | 断言队列里还有键入项，前提是断言前消费未发生；实测 200 次中 17 次失败（见 2.2） |
| `paste::drain_to_writer_tests::drain_to_writer_stops_when_queue_full_and_keeps_remainder` | **是** | 用"每次 write sleep 200ms"的慢写端制造满队列，填充与消费仍在赛跑 |
| `app::tests::reader::full_channel_retries_and_delivers_all_without_deadlock` | 否 | 消费者虽 sleep 1ms，但断言只看最终字节数与顺序 |

### 2.2 施压与复现

| 条件 | 对象 | 结果 |
|---|---|---|
| `taskset -c 0` + 同核 6 个忙循环，200 次 | 旧断言 `paste_queue_respects_cap` | **0 次失败**（Linux 未复现） |
| 同上，并行 `--test-threads=4` 跑全 lib 套件 200 次 | 中间态（已换受控 sink、未加"等在途"前置） | **17 次失败**（ctrl_c 与 drain_to_writer 两条） |
| 生产者每次入队延迟 300µs（等价 Windows 上更大的调度/唤醒开销） | 旧断言 | **20/20 失败**（`accepted=4194304`，从未被拒绝） |

根因（已验证事实）：竞态源于"断言读取的是**队列实时状态**"，而写线程会并发 pop。写线程一旦取走首块并进入 `write`，`queued` 立即少一块，随后 `try_enqueue_paste` 可能再次成功、Ctrl-C 前的键入项可能已被写走、`ESC[201~` 可能已插在队首。产品代码的记账/唤醒/`retain` 语义均正确，**不是产品缺陷**，因此只改测试。

### 2.3 修复与验证

新增 `writer::test_support::GatedSink`（`write` 在测试放行前阻塞），并统一加"先等写线程取走首块（`queued_bytes()==0`）再断言"的前置，使断言只依赖队列自身状态与返回值；`paste.rs` 的 `SlowSink` 一并替换。

| 条件 | 修复后 |
|---|---|
| 并行 `--test-threads=4` 跑全 lib 套件 200 次 | **0 失败** |
| `taskset -c 0` + 6 噪声 200 次 | **0 失败** |
| 生产者延迟 300µs 20 次 | **0 失败**（同一条件旧断言 20/20 失败） |

## 3. t3 release 真实路径的停顿门禁（case43）

### 3.1 设计

- 全程 `target/release/app`，真实路径：`xclip` 写 1MB → 右键（`click --delay 0 3`）→ `do_paste` → `paste_from_text`；不使用 `APP_TEST_PASTE`（debug 专属）。
- 正向证据：前台 `sleep 18; head -c 2000000 > 文件`，读回落盘字节数证明"粘贴确实到达 tty"。
- 主线程判据：**心跳打印是否继续**——主线程被写阻塞时 `about_to_wait` 不执行，日志里没有任何 `loop_gap_ms` 行（NA）。
- 驱动：单进程链式 xdotool，`mousemove_relative` 左右交替（单向漂移会把指针带出窗口→ 空转），每步 `sleep 0.04`。

### 3.2 十次分布（cpu/gpu 各 10 次）

| 后端 | min | 中位 | max | ≥80ms | ≥100ms |
|---|---|---|---|---|---|
| cpu `loop_gap_max` | 99ms | 130ms | 896ms | 10/10 | 9/10 |
| gpu `loop_gap_max` | 10ms | 11ms | 12ms | 0/10 | 0/10 |
| 缩放生效延迟（20 次） | 14ms | 18ms | 22ms | 0 | 0 |

### 3.3 为什么 <100ms 未达成（定位）

- 驱动本身没问题：300 步 `sleep 0.04` 实测 12051ms，符合预期。
- **对照组（不粘贴、同样驱动）CPU 侧就出现 114 / 126 / 455ms**；正式 10 次运行 CPU 稳定 99–130ms，全套 e2e 并发负载下 1009–1012ms。→ 该指标测的是"事件到达间隔"，混入了 CPU 后端单帧渲染成本，不是写阻塞。
- 写阻塞的量级完全不同：同步写实测 ≈12.9s，且日志直接断流（NA）。

### 3.4 门限与证伪

- 数值门限取 **2000ms** + 必须有心跳打印（NA 直接判失败）。写阻塞为 NA 或 ≈12900ms，判别力不受影响；若坚持 100ms，需在 app 内加"输入事件到达→处理完成"的内建计时（未实现，超出授权）。该取舍已按"待验证假设"登记 DECISIONS，附推翻条件。
- 证伪（已验证）：worktree `/tmp/wt4j3`（独立 `CARGO_TARGET_DIR`）里删除 `NativePty::take_writer`（不入仓库），同一流程 `loop_gap_max=NA` → **红**；主干二进制同流程 125ms → **绿**。
- 注意：缩放几何由 WM 立即应用（证伪组里缩放延迟仍 22ms），所以"缩放生效"只能证明窗口可缩放，不能作为主线程响应性证据。

### 3.5 驱动两个坑（已验证）

`xdotool click` 默认 `--delay 100`（按下与释放间隔 100ms），与门限同量级，会造成假停顿 → 必须 `--delay 0`；`mousemove_relative` 的负偏移必须写成 `mousemove_relative -- -1 0`，否则被当成选项报错、整条链中断。

## 4. t4 字体加载内部打点

`font_ms` 区间内实际步骤：`fontdb query → db.face → 文件读取 → fontdue 解析 → Font::clone 入缓存`。新增 `font_face_ms`（此前唯一未覆盖步骤）与每组起止 `font_group_begin_ms`/`font_group_ms`（带 `used_style`/`fallback`，区分主字体面与 style 回落面）。Linux release 5+5 次：

| 组 | 分步之和 | `font_group_ms` | 差 |
|---|---|---|---|
| 0 Regular | 28.61ms | 28.78ms | 0.17 |
| 1 Bold | 15.65ms | 15.80ms | 0.15 |
| 2 Italic | 17.41ms | 17.57ms | 0.16 |
| 3 Bold Italic | 14.40ms | 14.56ms | 0.16 |

判据 ≤5ms 通过（cpu 与 gpu 几乎相同）。四组合计 ≈77ms。

**Windows 应关注**：`face_ms` 与 `read_ms`（Linux 上分别为 0.00 / 0.2ms），以及 `fallback` 标记（回落会重复解析）。3H 的每组 139–155ms 中 parse 22–30、cache 7–9、read≈1，缺口 ≈100ms/组，最可能在 `db.face`。

## 5. t5 README「已知平台限制」

新增四条，每条标依据：`ESC[7m` 被 conhost 物化为 `ESC[30m`+`ESC[47m`（PHASE3G-WIN §4.1/§4.2）；`OSC 104` 两种 ConPTY 都不到达（同 §4.1 s4）；ConPTY 吞括号粘贴标记与 SGR 鼠标序列、conhost 把折行物化为 CRLF（WIN-NOTES/PHASE3D-WIN）；GPU 后端仅 Linux（`crates/render/Cargo.toml` 的 `cfg(target_os = "linux")` 段已复核）。

## 6. t6 长跑复测：未达判据

15 分钟/后端，release，:99，负载含 1MB 粘贴、每 8 轮一次"粘贴中 Ctrl-C 取消"、缩放、选区拖动、滚动条拖动、滚轮上滚回底。

| 项 | CPU | GPU | 判据 |
|---|---|---|---|
| RSS 预热末 → 峰值 | 236212 → 380728KB（**+61.2%**） | 411372 → 550460KB（**+33.8%**） | ≤10% ❌ |
| fd | 13 恒定 | 54 恒定 | 不单调增长 ✅ |
| 线程数 | 4 恒定 | 4 恒定 | 恒定 ✅（=4F 的 3 + 写线程） |
| panic | 无 | 无 | 无 ✅ |
| SIGTERM 退出 | ≤10s | ≤10s | 限时 ✅ |
| `loop_gap` 最大 | 25ms | 23ms | 观测项 |

**隔离实验（已验证事实，各自单独跑都平坦）**：`seq 1 800` 每 5s → 219MB 恒定；escapes（OSC 10/11/110/111 + SGR）→ 219.6MB；滚轮上滚回底 → 215–217MB；resize+选区+滚动条 → 214–216MB；**只做 1MB 粘贴 → 220.8MB 恒定**；粘贴中 Ctrl-C → 226–230MB 无趋势；300k 行一次性输出 → +5MB 后平坦。

**增长只在"每轮 `seq 1 800` 叠加 1MB 粘贴"时出现**（+14–20MB/min）。在 worktree 里给 `VtTerminal` 加 `set_scrollback_max_bytes(Some(16MB))` 后同样条件仍增长（+20MB/min）→ **排除无界 scrollback**。

根因未定位（待验证假设，置信度低）：剩余嫌疑是"大批量时序下的分配器保留（arena/fragmentation）"或"Xlib/winit 待处理事件队列膨胀"。推翻条件见 DECISIONS。按任务要求，r6 判据不满足即不打 t6 tag、不打 `phase4j-done`。

## 7. 未解决问题

1. **t6 的 RSS 增长根因未定位**（最重要）：已把范围压到"seq 输出 + 大粘贴"的组合，排除了 scrollback 上限、单分支、fd/线程泄漏；需要 `MALLOC_ARENA_MAX`、`malloc_trim` 注入或 heaptrack 级别的取证才能定性。
2. **case43 的 <100ms 未达成**：已定位为 CPU 单帧渲染混入指标；若必须 100ms，需要 app 内建输入延迟统计（未实现）。
3. **字体面成本在 Windows 上的归属未核实**：本轮只补点，需 Windows 取数（重点 `face_ms`）。
4. **粘贴中 Ctrl-C 后已写入 tty 队列的字节仍会被 shell 执行**（4H 已知）：要彻底清掉需 TIOCSTI 或应用层粘贴事务。

## 8. Windows 3J 交接

1. 用 t4 新增点复取首帧：重点 `font_face_ms`、`read_ms`、每组 `fallback` 标记。
2. `take_writer` 已在 Windows 落地（合并带入）：`cargo test -p app --lib writer` 与 `paste` 两组单测应全过；若 Windows 上仍偶发失败，说明还有其他时序断言，按 t2 的"受控 sink + 等在途"模式处理。
3. t6 的 RSS 增长在 Windows 上是否同样出现（CPU 后端）：若出现，用同样的隔离实验（单分支 vs 组合）定位，并优先看 ConPTY 输入管道的缓冲是否随粘贴累积。

## 9. 手动清单（≤3）

1. office-pc 上粘贴 10MB 到忙窗口，确认界面不卡死（Linux 已从 12.9s 降到无阻塞）。
2. Windows 侧复取首帧分段（重点 `font_face_ms`），确认那 ≈400ms 的构成。
3. 长时间（≥1 小时）连续大量粘贴后观察任务管理器内存走势，用于判断 t6 的 RSS 增长是否会在真实使用中失控。
