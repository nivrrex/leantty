# PHASE4I-LINUX：Windows 3G 合并 / Ctrl-C 截断括号粘贴 / 回显停顿归因 / OSC 4·104

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3g-win-done-2` | 已合并（tag `phase4i-t1`），共用代码零改动 |
| t2 Ctrl-C 截断括号粘贴 | **已复现（5/5 卡死）并修复**（tag `phase4i-t2`） |
| t3 回显开启大粘贴停顿 | **release 未复现 ≥100ms，不实施**，未打 tag |
| t4 OSC 4 / OSC 104 | app 层均生效，CPU/GPU 一致，回退可红（tag `phase4i-t4`） |

## 1. t1 合并证据

`git ls-remote ali`（合并前实测）：

```
fddc97ba1b5854ca3cf73f3dbf3eb241ecc4266d	refs/heads/main
e3a16ef826da42d2e007ce62935bd11e8c844c65	refs/heads/win
52dad280fa3f885498869b25284ffe17be13dcdd	refs/tags/phase3g-win-done
32fb9ee2712269dfbae762abac3c8cf77a0b323f	refs/tags/phase3g-win-done^{}
43e38fdbe0d728c2957f7aab71e62e15208970d3	refs/tags/phase3g-win-done-2
e3a16ef826da42d2e007ce62935bd11e8c844c65	refs/tags/phase3g-win-done-2^{}
```

两者都是附注标签：`phase3g-win-done` → `32fb9ee`，`phase3g-win-done-2` → `e3a16ef`（= win 头）。
`git merge-base --is-ancestor 32fb9ee e3a16ef` 为真，即 `-2` 更晚 → **合并 `e3a16ef`**（与预期一致）。

Windows 侧自 `afb2444` 起的 95 个文件里，非 Windows 专属目录的改动仅：

| 文件 | 变更 |
|---|---|
| `.gitignore` | 新增 `/vt-verify-win/probe_inv/target/` |
| `PHASE3F-B-WIN.md` / `PHASE3G-WIN.md` | 新增 Windows 报告 |
| `docs/WIN-NOTES.md` | Windows 专属文档 |

无共用代码改动 → `--no-ff` 合并，合并后 155 测试 0 失败、e2e 96/96。

## 2. t2 Ctrl-C 截断括号粘贴

### 2.1 复现（release，真实路径）

场景：`bash --norc --noprofile`（bash 5.2.15，glibc 2.36，括号粘贴由 readline 默认开启），
xclip + 右键粘贴 5MB（4550000B）文本，0.6s 后注入 Ctrl-C，再键入
`echo WEDGE_OK_$((1+1)) > <文件>` 并回车。

| 版本 | 结果 | 证据 |
|---|---|---|
| 修复前 | **0/5** | 等待 30s 仍无探针文件；写线程记录 `ctrl_c_drop_paste bytes=1048576`（队列中 1MiB 未写数据被丢弃，含 `ESC[201~`） |
| 仅补发结束标记 | **0/5** | 同上，`ctrl_c_restore_paste_end` 已打印 |
| 补发 + app 侧取消粘贴 | **5/5** | 探针文件内容 `WEDGE_OK_2` |

### 2.2 根因（任务假设需修正）

- 任务假设"结束标记丢失 → readline 卡住"**部分成立但不完整**：只补发 `ESC[201~` 仍 5/5 卡死。
- 判定实验：Ctrl-C 后 1s 再发一次 Ctrl-C，日志出现**第二条** `ctrl_c_drop_paste bytes=1048576`
  → 说明 **app 侧 `pending_paste` 在 Ctrl-C 后继续把粘贴回灌写队列**，粘贴根本没有停止。
- 佐证：孤立 python-pty 实验（直接写 3.5MB 括号粘贴、不发结束标记，随后 ^C）中 bash
  **0.5s 内自恢复并执行探针**（有无结束标记都一样）——bash 不会因缺结束标记永久卡住。

### 2.3 修复（两处，均不触碰 trait 与 vendor）

1. **写线程**（`crates/app/src/writer.rs`）：Ctrl-C 丢弃粘贴块时，若起始标记已开始写、
   且正在写的那一块不带结束标记，则在 0x03 **之前**补发 `ESC[201~`（同一临界区入队，
   不等待写出）。状态位：`open_paste`（起始已写出、结束未写出）、`in_flight_ends`。
2. **app**（`crates/app/src/lib.rs`）：Ctrl-C（0x03）时清空 `self.pending_paste`，令粘贴整体作废。

行为契约（工程判断，置信度中）：Ctrl-C = 取消整个粘贴——已写出的字节无法收回，队列内
未写出的全部丢弃并补结束标记。推翻条件：若要求"中断前台但粘贴继续"，则应去掉这两处。

### 2.4 门禁

| 项 | 结果 |
|---|---|
| e2e `case41`（cpu） | PASS（`dropped=1048576B`，探针 `WEDGE_OK_2`） |
| GPU 轮同用例 | PASS |
| 回退两处修复 | 复现 0/5 → 红 |
| 单测 +4 | 起始已写→补尾；起始未写→不补；在途块自带结束标记→不补；多次丢弃→只补一次 |
| 顺带修 | `drain_to_writer` 用例的并发竞态断言（原断言假设"一次恰好接受满上限"） |

## 3. t3 回显开启时大粘贴的主线程停顿

### 3.1 量化（release，真实路径）

新增观测（release 可用、默认关闭）：`APP_PERF_RESET_MS`（观测窗口重置）、迭代级
`feed_ms_max` / `feed_bytes_max` / `writer_queued_max`，并同时输出到 5 秒
`[perf] loop_gap_ms=` 行（慢渲染、少帧场景下 frames 行可能不出现）。

| 场景 | 1MB | 5MB |
|---|---|---|
| bash（括号粘贴，回显开，正常读取）×3 | 最大迭代间隔 23–28ms | 7–11ms |
| `/bin/sh` 回显灌回 + `head -c` 读取 5.07MB（复刻 4H case39） | — | 最大迭代间隔 **13ms**；`feed_ms_max=0.6ms`/次；单次最多喂 44689B；`chan_depth_max=1`；写队列满 1MiB；feed 2.2MB/s；收到 5066752/5070000 字节 |

### 3.2 归因（已验证事实）

**4H 的 21s 是 debug 构建的产物**——4H 的 case39 用的是 `target/debug/app`。
debug 同场景实测：单次迭代 `feed_ms_max=12447ms`（该迭代喂入 254912 字节），
停顿 12595ms 中 **12447ms 就是这一次 `vt.feed`**，即"单次迭代一次性喂太多 × debug 解析慢"。

release 下同一批数据 feed 仅 0.6ms，停顿 ~13ms → **低于 100ms 判据，不实施时间预算**，
按任务要求**不打 t3 tag**，仅提交测量与 DECISIONS。

### 3.3 case38 十次分布（修正驱动后）

原驱动有两处测量缺陷（已验证）：
1. 每 40ms **派生一次 xdotool** 实测耗时约 90ms → gap 变成驱动周期（88–95ms，偶发 115ms 假红）；
2. 驱动若在粘贴触发（2.5s）之后才开始，第一个 gap 是"等事件"的空闲间隔（≈90ms）。

改为**单进程链式** `xdotool move sleep move …` 并紧接命令后启动后：

```
case38 × 10: 19 10 20 21 19 21 10 17 20 7 ms
min 7 / median 19 / max 21 / ≥80ms 次数 0
```

## 4. t4 OSC 4 / OSC 104（app 层）

新增 e2e `case42`（cpu）与 GPU 轮同用例。配置 `normal.red = #d32f2f`：

| 步骤 | 期望 | CPU 实测 | GPU 实测 |
|---|---|---|---|
| `OSC 4;1;rgb:aa/bb/cc`（BEL）后 `ESC[31m` 反色实心块 | #AABBCC | **2729 px** | 2729 px |
| `OSC 104` 后同一块 | 配置红 #d32f2f | **5459 px** | 5459 px |
| 同期内置默认 #CC6666 | 0 | **0 px** | 0 px |

**结论**：OSC 4 与 OSC 104 在 Linux 的 app 层均生效，3F-b 的"OSC 4 在 app 层不生效"
在 Linux 上不成立（Windows 差异应归因 ConPTY，待 Windows 复核）。

**证伪（已验证）**：把 VT 创建从 `new_with_palette` 回退为 `new` → OSC 4 **仍生效**
（2729px，libghostty 自行处理），但 OSC 104 之后回到内置默认 **#CC6666 5459px、配置红 0px**
→ 用例变红；恢复后变绿。故该用例的有效判据是"OSC 104 后是否回到**配置色**"。

**取样方法（已验证）**：文字笔画在抗锯齿下精确色像素只有个位数，不能作门禁；改用
`ESC[31m ESC[7m` + 10 空格的反色实心块（≈1280px），阈值 500。

## 5. 未解决问题

1. **已写入 tty 队列的字节在 Ctrl-C 时仍会执行**（与 4H 相同）：本修复只丢弃"尚未写出"
   的部分并补结束标记；已进入内核 tty 缓冲的字节仍会被 shell 消费。彻底解决需 TIOCSTI
   或应用层粘贴事务，超出本任务授权。
2. **Ctrl-C 语义是"取消整个粘贴"**：若用户希望粘贴在中断后继续，需要改回（见 DECISIONS 推翻条件）。
3. **case38 的判据基于 debug 构建**（`APP_BIN=target/debug/app`，因需要 `APP_TEST_PASTE`
   与 `APP_DUMP_SNAPSHOT` 钩子）：其 7–21ms 与 release 的 8–13ms 同量级，但严格说
   release 侧的写线程停顿证据来自 t3 的测量，而非 case38。
4. **Windows 侧的 Ctrl-C 行为未验证**：Windows 的 `take_writer` 返回 `None`，仍是同步写，
   不存在本修复涉及的队列丢弃路径，但 ConPTY 下的括号粘贴中断行为未核实。

## 6. Windows 交接（3I）

1. **OSC 4/104**：在 Windows 上跑同场景（`normal.red=#d32f2f`，OSC 4 → 反色块，OSC 104 →
   反色块），预期 OSC 4 生效、OSC 104 被 ConPTY 丢弃 → 第二块仍为 #AABBCC。若第二块回到
   #d32f2f，则 3G 的"OSC 104 被 ConPTY 丢弃"需要重新取证。
2. **Ctrl-C 截断括号粘贴**：Windows 走同步写路径（无写队列丢弃），Ctrl-C 语义由 ConPTY
   传递；若观察到提示符卡住，应按 t2a 的方式（探针命令落盘）先取证再判断是否需要类似处理。
3. **首帧分段**（承 4H）：仍建议用 `fontdb_ms`/`font_seg_ms`/`event_loop_ms` 等点复取。
4. `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 本轮通过。

## 7. 手动清单（≤3）

1. office-pc / Windows 上：括号粘贴大段文本（≥1MB）后立刻 Ctrl-C，确认提示符能立刻回来
   且后续命令可执行（Linux 已从 5/5 卡死变为 5/5 正常）。
2. Windows 上复验 OSC 4 → OSC 104 的颜色回退行为（见 6.1），确认是否属 ConPTY 限制。
3. 真实开发机上（非 Xvfb）用 5–10MB 粘贴到正在编译的窗口，确认界面不卡顿（本轮 Xvfb
   release 实测最大停顿 13ms）。
