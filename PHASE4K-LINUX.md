# PHASE4K-LINUX：输入回底修复 / RSS 增长根因 / 长跑复测 / 平台限制

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论摘要

| 任务 | 结果 |
|---|---|
| t1a 合并 `phase3i-win-done` | 已合并，共用代码零改动 |
| t1b 滚到历史后键入不回底 | **根因：该能力从未实现**；已修复并加门禁（tag `phase4k-t1b`） |
| t2 RSS 增长根因 | **app 自有结构：粘贴积压无界**；已修复，最小复现由 7.5MB/min 变为持平（tag `phase4k-t2`） |
| t3 长跑复测 | **通过**：CPU +1.6%、GPU +1.4%（tag `phase4k-t3`） |
| t4 Windows Ctrl-C 限制 | README 已加，标依据（tag `phase4k-t4`） |

> tag 说明：`phase4k-t1` 被误建在 t1a 合并提交（`2ea6f31`）上——当时提交信息里的引号被 shell
> 吃掉导致 commit 失败、而 tag 仍按 HEAD 创建。按"不得 force"的规则保留该 tag 不动，t1b 的
> 正确提交另建 `phase4k-t1b`（`b230e55`）。两者都在 main 的历史上，后者是前者后代。

## 1. t1a 合并证据（ls-remote 实测）

```
483e0dc9ba79292cab35820075f236234f1c7983	refs/heads/main（合并前）
9ce7daab85725ecb02cbbff74d742c2b52a8ba08	refs/heads/win
1de3269e7d047d7515f2d3f5b75bcda2f5d05bb5	refs/tags/phase3i-win-done
9ce7daab85725ecb02cbbff74d742c2b52a8ba08	refs/tags/phase3i-win-done^{}
```

- tag 是附注标签，`^{}` 解引用为 `9ce7daa`，与 `refs/heads/win` 相同 → 合并 `9ce7daa`。
- **Windows 报告与最终回复的 hash 差异**：`PHASE3I-WIN.md` 记录的 push 结果是 `73fc635..a642333`，
  那是"记录 hash"提交**之前**的中间态；最终 win 头是 `9ce7daa`（该提交只补记 hash）。
  以 ls-remote 实测为准。
- 合并结果相对 main 的共用/受控范围**只有** `PHASE3I-WIN.md`（新增）与 `docs/WIN-NOTES.md`；
  `crates/` 无差异（`windows.rs` 的 `take_writer` 已在 4J 合并带入；`paste.rs` 的差异方向是
  win 落后于 4J 的测试改写，合并后保留 main 版本）。`cargo check --target x86_64-pc-windows-gnu
  --workspace --tests` 通过、依赖树 `wgpu`/`pollster` 命中 0。

## 2. t1b：滚到历史后键入不回底

### 2.1 规格（已写入 DECISIONS）

视口不在底部时，**向 PTY 发送内容**的操作应使视口回到底部：可打印字符、编码按键
（Enter/Backspace/Tab/方向键等）、粘贴（右键/Ctrl+Shift+V/配置绑定，含括号粘贴）、IME 上屏。
**不**触发回底：仅修饰键、复制动作（含 Ctrl+Insert 的 Copy）、滚轮/滚动条拖动、选区、缩放。
视口在历史位置时新输出到来视口保持不动（既有 auto-follow 语义不变）；备用屏与鼠标上报模式
行为不变。

### 2.2 二分结果（判别用例 `scripts/e2e/bisect_input_follow.sh`）

用例流程：输出 300 行 → 对自身窗口滚轮上滚 10 格 → 键入 `echo MARK_$((1+1))` + Enter →
判定。观测：优先用视口网格里是否出现 `MARK_2`（`APP_DUMP_SNAPSHOT`，debug 钩子；历史 tag 上
同样存在），无 dump 时回退像素判据（键入后画面相对"上滚视图"是否发生大变化）。

| 版本 | 结果 | 证据 |
|---|---|---|
| phase2b-done | FAIL | dump：无 MARK_2；AE(B,C)=28 |
| phase3c-done | FAIL | AE(B,C)=28 |
| phase4b-done | FAIL | AE(B,C)=28 |
| phase4f-done | FAIL | AE(B,C)=84 |
| phase4g-t1 | FAIL | AE(B,C)=84 |
| phase4h-t3 | FAIL | AE(B,C)=84 |
| phase4i-done | FAIL | dump：无 MARK_2；AE(B,C)=84 |
| 修复前 HEAD | FAIL | 同上 |
| **修复后 HEAD** | **PASS** | dump 末行出现 `MARK_2` |

**结论：该能力在任何历史版本中都不存在——是未实现，不是回归。**

### 2.3 根因（函数与位置）

- `crates/app/src/lib.rs::about_to_wait` 的 auto-follow 分支：
  `if self.drained_any && self.follow_bottom { vt.scroll_viewport(Bottom) }` —— **只在"有新输出
  到达"时回底**。
- `on_mouse_scroll`（滚轮）把 `follow_bottom` 置为 `offset_from_top >= sb` —— 上滚后为 `false`。
- 所有输入路径（`KeyRoute::Text` / `KeyRoute::Encoded` / `on_ime_event` / `paste_text`）此前
  **只写 PTY，从不回底**。4H 引入写线程没有绕过任何既有触发点，因为触发点根本不存在。

### 2.4 修复

- 纯决策：`crates/app/src/follow.rs::should_follow(InputEvent, alt_screen, mouse_reporting)`
  ——只有 Printable/EncodedKey/Paste/ImeCommit 且非备用屏、非鼠标上报模式才回底。
- 唯一入口：`AppHandler::follow_viewport_before_input(ev)`——决策为真且当前不在底部时，
  置 `follow_bottom=true`、`offset_from_top=sb_rows()`、`scroll_viewport(Bottom)`、请求重绘；
  已在底部时无副作用（不产生额外帧）。
- 调用点：可打印字符键入、编码按键、IME 提交、`paste_text`（粘贴同步/有界入队入口）。

### 2.5 连带缺陷（门禁暴露）

默认绑定 `Ctrl+Shift+C` 此前**从未命中**：winit 对带 Shift 的组合键上报 `Character("C")`，
而内置绑定写 `"c"`，匹配为精确比较 → 该键被当普通编码键送进 PTY（既没复制，也会把视口
带回底部）。修复：`key_matches` 对单字符键做大小写无关比较，Named 键仍精确匹配；
单测 `key_matches_is_case_insensitive_for_single_chars`。

### 2.6 门禁与证伪

`case44`（release，cpu 与 gpu 各一轮），观测"滚动条拇指底部占比"：

| 状态 | cpu | gpu |
|---|---|---|
| A 底部 | 1.000 | 1.000 |
| B 上滚后 | 0.863 | 0.863 |
| 键入后（应回底） | 1.000 ✓ | 1.000 ✓ |
| 粘贴后（应回底） | 1.000 ✓ | 1.000 ✓ |
| 上滚期间新输出 | 0.901→0.901 ✓ 不动 | 同 |
| 复制动作后 | 0.901 ✓ 留在历史 | 同 |

**证伪**：把 `should_follow` 临时改为恒 `false`（等价回退修复）→ 键入后 0.855、粘贴后 0.637
→ 两条 FAIL；恢复即 PASS。

## 3. t2：RSS 增长根因

### 3.1 最小复现（`scripts/e2e/rss_probe.sh`，默认 6 分钟）

负载：每轮 `seq 1 800` + 一次 1MB 真实路径粘贴（xclip + 右键），release，cpu，每 30s 采样。
**修复前**：RSS 215468 → 272836KB（≈**7.5MB/min**，4 分钟）。

### 3.2 映射分类（判别分配器 vs 应用结构）

| t | VmRSS | RssAnon | RssFile | VmData | heap | anon mmap | file | Xvfb RSS |
|---|---|---|---|---|---|---|---|---|
| 30s | 222116 | 208508 | 9620 | 242300 | 0 | 222292 | 0 | 73672 |
| 120s | 246856 | 233248 | 9620 | 268304 | 0 | 246936 | 0 | 73672 |
| 240s | 272836 | 259228 | 9620 | 294336 | 0 | 272992 | 0 | 73736 |

- 增长**全部**在匿名 mmap（`heap=0`、`file=0`），`VmData` 同步增长。
- Xvfb 的 RSS 恒定 73.7MB → **增长不在 X 服务器**（排除"事件积压到 X 侧"的表象）。

### 3.3 根因

app 的 `[perf]` 结构观测（本轮新增：`sb_rows` / `glyphs` / `fb_bytes` / `pending` / `wq_cap`
`wq_len` `wq_bytes`）在同一时刻给出：

```
sb_rows=394 glyphs=29 fb_bytes=3931200 pending=0/0B wq_cap=1024 wq_len=770 wq_bytes=49000904
```

→ **写线程队列积压 770 项 / 49MB**，而 scrollback 只有 394 行（与 4J 的结论一致：
`set_scrollback_max_bytes` 不是关键）。原因是 4H 为避免"键入丢失"把 `enqueue_typed` 设计成
无上限，而 <1MB 的粘贴走的正是 `paste_text → enqueue_typed`；当接收端（shell 逐行执行被粘贴
的命令）消费慢于粘贴生产时，待写数据无界增长。

假设排除：

- A（分配器保留）：增长与待写字节一一对应（49MB 队列 ↔ ~57MB RSS 增长），且给队列加限后
  积压**整体转移到 `pending_paste`（49MB）**，说明缺的是"积压总量"上限，不是分配器行为。
- B（libghostty 页分配）：`sb_rows` 只有数百行；4J 已实测 16MB scrollback 上限无效。
- C（X 事件积压）：Xvfb RSS 恒定；与是否注入鼠标事件无关。

### 3.4 修复

- `paste::PASTE_BACKLOG_CAP = 12MiB` + 纯函数 `accept_within_backlog(bytes, backlog, cap)`；
  12MiB 的取值必须容纳**单次 10MiB 粘贴**（既有保证，case25 用 10×1024×1024 字节）。
- `paste_text`：先按积压上限截断 → 走**有界**粘贴入队（`try_enqueue_paste`，1MiB 写队列帽）
  → 剩余进 `pending_paste`（由既有预算化排空 + `request_redraw` 自驱动）。
- `paste_from_text` 的 ≥1MB 分块路径同样受该上限约束。
- 超限丢弃尾部并打印 `[perf] paste_dropped bytes=N backlog=B cap=C`（行为决策，见 DECISIONS
  与推翻条件）。

**效果（同一最小复现，4 分钟）**：RSS 215468 → 231952KB 后从 t=150s 起**完全持平**；
`pending` 到顶 12MiB、`wq_bytes` 到顶 1MiB。

### 3.5 门禁

`case45`（release）：连续 14 轮"seq 800 + 1MB 粘贴"灌入慢消费者，断言写队列 ≤ 1MiB+64KiB、
`pending ≤ 12MiB`，且**必须观测到积压**（全 0 视为"负载未产生积压"→ FAIL，防止空过）。
实测 `wq=1048676B`、`pending=11B`～12MiB。

## 4. t3 长跑复测（判据不变，通过）

15 分钟/后端，release，:99，负载含 seq 输出、1MB 真实粘贴、每 8 轮一次"粘贴中 Ctrl-C"、
缩放、选区拖动、滚动条拖动、滚轮上滚回底。

| 项 | CPU | GPU | 判据 |
|---|---|---|---|
| RSS 预热末→峰值 | 219656→223140KB（**+1.6%**） | 394076→399400KB（**+1.4%**） | ≤10% ✅ |
| fd | 13 恒定 | 54 恒定 | 不单调增长 ✅ |
| 线程数 | 4 恒定 | 4 恒定 | 恒定 ✅（=4F 的 3 + 写线程） |
| panic | 无 | 无 | ✅ |
| SIGTERM 退出 | 限时 | 限时 | ✅ |
| loop_gap 最大 | 22ms | 26ms | 观测项 |

RSS 曲线（CPU）：180s 219656 → 360s 219264 → 540s 219800 → 720s 220260 → 900s 222748KB。

**加载器修正（非产品缺陷）**：前两次复测 app 在 t=423s / t=895s 提前退出——非交互 `sh` 里
`sleep 20` 与 shell 同属前台进程组，Ctrl-C 连带打死 shell。改为 `SHELL=/bin/bash` + 配置
`[terminal.shell] args=["--norc","--noprofile"]`（交互式 bash 有作业控制）后两轮跑满。
`set -m` 包 `sh -c` 无效（dash 不改变 tty 前台进程组）。

## 5. t4 README：Windows Ctrl-C 限制

新增一条：Windows 上粘贴大文本后按 Ctrl-C，app 侧丢弃未写出数据有效（3H：5MB 粘贴中
Ctrl-C 后子进程只收到 123130 字节），但已进入控制台输入缓冲/PSReadLine 的部分无法撤回，
之后键入的命令会拼在已送达文本之后（5MB 0/5、100KB 0/2、单次 debug 90s 未恢复；
对照直接输入 263ms 生效）；Linux 无此限制。依据：`PHASE3I-WIN.md` §4。未改动 Windows 专属文件。

## 6. 未解决问题

1. **12MiB 积压上限的产品含义**（行为决策）：极端场景（持续把 1MB 粘贴灌进慢消费者）会丢弃
   粘贴尾部并记日志。若要"无损"，需要背压或磁盘暂存，属产品取舍。
2. **case44 的观测量依赖滚动条**：若将来支持隐藏滚动条，需要换观测（例如读快照行号）。
3. **`xdotool key --window` 不能用于组合键**：合成事件不带修饰键状态，会把 `Ctrl+Shift+C`
   当普通 `C` 送进 PTY——e2e 里一律对焦点窗口发送。此坑已写入用例注释。

## 7. Windows 3K 交接

1. t1b 的回底语义在 Windows 上同样适用（纯 app 层逻辑，无平台分支）：`cargo test -p app --lib follow`
   与 `key_matches` 单测应通过。
2. 粘贴积压上限在 Windows（ConPTY，`take_writer` 已实现）同样生效；若 Windows 上粘贴大文本
   出现"少了一截"，先看是否打了 `[perf] paste_dropped`，那是上限触发的正常行为。
3. Ctrl+Shift+C 的绑定大小写修复对 Windows 同样重要（此前的实际表现是"按了没复制"）。

## 8. 手动清单（≤3）

1. 真实使用中把视口滚到历史再键入/粘贴，确认会回到最新输出（这是本次修复的用户可见点）。
2. 连续粘贴大文本到忙碌的 shell，观察日志是否出现 `[perf] paste_dropped`，判断 12MiB 上限
   是否符合预期。
3. Windows 侧确认 Ctrl+Shift+C 现在能复制（此前该键被当普通字符送进 PTY）。
