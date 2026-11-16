# PHASE4T-LINUX：合并（无新功能提交）/ 定时依赖审计 / Windows 存活检查

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 Windows | 自 `phase3q-win-done` 起**无新功能提交**，仅 3Q 补记 hash 的 `1791c76`（已合并） |
| t2 定时依赖审计与测试 | 8 项行为在 `ControlFlow::Wait` 下**全部仍工作**；同时**修掉一个 4Q 引入的真缺陷** |
| t3 Windows 存活检查 | **已实施**（1 秒间隔，仅 `cfg(windows)`），单测 5 条；Linux 保持纯事件驱动 |

**tag 说明**：`phase4t-t1` 与 `phase4t-t2` 都指向 `1f9c3a8`。原因：t1 的合并只带入
`PHASE3Q-WIN.md`（+8/−1，报告 hash），而 t2 的代码改动与它相邻提交；门禁在 `1f9c3a8` 这棵
树上一次性跑过（`gates_ok` 只能是当前 HEAD），故两个 tag 指向同一提交。t3 在 `05b05b9`。

## 1. t1 合并证据（ls-remote 实测）

```
f62e45692d3a36484e536017928ebc8de1b851b9	refs/heads/main（合并前）
1791c76d470adf82c37c35686f0f9ebc478c763b	refs/heads/win
85b842bb6e62fa43283b8b3f5a7c4d003a7dbfd0	refs/tags/phase3q-win-done
cc43cf03529fd123a8a26cb964c4640f71edd780	refs/tags/phase3q-win-done^{}
```

- `phase3q-win-done` = `85b842b`（附注标签）→ `^{}` = `cc43cf0`（上一阶段已合并）。
- `refs/heads/win` = `1791c76` = `cc43cf0` 之后**唯一**的提交
  （`3Q-win: record push/tag hashes in the report`，仅改 `PHASE3Q-WIN.md`，+8/−1）。
- 结论：自 `phase3q-win-done` 起**没有新的功能/代码提交**。为让 win 头成为 main 的祖先
  （避免后续再出现"补记提交未合并"），本次把该报告提交以 `--no-ff` 合并；`crates/` 零差异。

## 2. t2a 定时/轮询依赖审计

| 行为 | 触发源 | `ControlFlow::Wait` 下是否仍执行 | 依据 |
|---|---|---|---|
| 拖选自动滚动（指针停在窗口外） | `CursorMoved` 置 `auto_scroll`；维护块推进 offset 后 `request_redraw()`（事件自驱动） | **是** | `on_cursor_moved`；`about_to_wait` 维护块 |
| 滚动条拇指拖动 | 鼠标按下/移动/释放事件 | **是** | `on_mouse_input` / `on_cursor_moved` |
| 双击 / 三击选词选行 | `ButtonPress` 事件 + `Instant::elapsed() < 300ms` 判定 | **是**（时间比较在事件内完成，不依赖定时器） | `start_selection` |
| shell 退出检测 | `about_to_wait` 里轮询 `Pty::try_wait`；Linux 上读线程在 `Ok(0)\|Err(_)` 时 `waker.wake()` | **是（Linux）**；Windows 见 §4 | `reader_body` / `about_to_wait` |
| OSC 0/2 窗口标题 | PTY 字节 → 唤醒 → 排空后维护块更新标题 | **是** | `about_to_wait` 维护块 |
| PRIMARY 选区 | 拖选结束 `commit_selection()` 写入；arboard 自行服务选择请求 | **是** | `on_mouse_input` 释放分支 |
| SIGTERM | 默认信号处置直接终止进程 | **是**（与事件循环无关） | 无处理器 |
| 粘贴排空 / 尾沿帧 / IME 钩子 | `request_redraw`（事件）与 `!due` 分支的 `WaitUntil` | **是** | `case25/28/30/32/39/47` |

## 3. t2b 测试矩阵（release，:99）

脚本：`scripts/e2e/timer_dep_matrix.sh <cpu|gpu>`（默认不进 run_all）。

| 项 | cpu | gpu |
|---|---|---|
| ① 拖选自动滚动（指针静止在窗口下缘外 2 秒） | 拇指 0.214→0.435→1.000、选区扩展 AE=6951、松开后停止 | 0.214→0.391→1.000、AE=6499、松开后停止 |
| ② 滚动条拇指拖出窗口再回来 | 松开后滚轮仍生效（AE=5237） | 同 |
| ③ 双击选词 / 三击选行 | `word1` / `word1 word2 word3` | 同 |
| ④a `exit` | app 250ms 内退出 | 同 |
| ④b 子 shell 被 SIGKILL | app 250ms 内退出 | 同 |
| ⑤ OSC 0 标题 | ≤100ms 生效 | 同 |
| ⑥ PRIMARY（空闲 5 秒后仍可读） | 一致 | 同 |
| ⑦ SIGTERM | 250ms 内退出 | 同 |
| **合计** | **8/8** | **8/8** |

**对照实现（把 4S 的 `ControlFlow::Wait` 去掉，恢复热循环）**：矩阵仍 **8/8**，但 `case50`
变红（空闲 12 秒新增 3 行统计、CPU tick 117、ctx 1499）→ 用例能区分两种实现，区分点正是
"空闲是否静默"。

## 4. t2c 修复：一个 4Q 引入的真缺陷

**现象**：GPU 后端下 ① 拖选自动滚动完全不推进（拇指恒 0.214）、⑤ OSC 0 标题不更新（恒 `app`）；
CPU 后端正常。

**根因**：`about_to_wait` 的**每帧维护块**（OSC 标题跟随、拖选自动滚动推进）原本位于渲染块
**之后**，而"GPU 空闲跳过"（4Q t3）与"CPU 空 damage 提前返回"都会 `return`，把维护块一并跳过。

**修复**：把这两段维护工作前移到渲染块之前——无论本帧是否真正绘制都会执行。修复后 GPU 的
①⑤ 转为 OK，CPU 不受影响（矩阵 8/8 两后端保持）。

**证伪**：把维护块移回渲染块之后 → GPU 的 ①（拇指恒 0.214）与 ⑤（标题恒 `app`）变红。

**已知限制**：既有 `case21`（拖选自动滚动）在 **GPU 后端**下只跨 23 行（阈值 >28 不达标），
CPU 后端 41 行通过。原因是该用例固定悬停 1.2 秒，而 GPU 每帧更贵、自动滚动步数更少。
`run_all` 中 case21 用 cpu 后端，故不影响门禁；`timer_dep_matrix` 的 ① 改用"是否持续推进 +
选区是否扩展"判定，两后端均通过。

## 5. t3 Windows 子进程存活检查

**判定依据**：退出检测 = `about_to_wait` 里轮询 `Pty::try_wait`（`lib.rs:~1858`）。Linux 上
读线程在 `Ok(0) | Err(_)` 时 `waker.wake()`（`lib.rs:~180-186`，注释即写明"否则 about_to_wait
不触发→try_wait 不轮询→子进程退出检测丢失"）→ **事件驱动，无需定时**。Windows 的 ConPTY
输出管道不会因子进程退出而 EOF（3D/3J 记录；本机无法复核 → 标"未核实"），若无唤醒源则轮询
不发生、窗口不关闭 → **需要低频存活检查**。

**实现**：

```rust
pub fn next_liveness_wake(now, child_alive, last_check, interval) -> Option<Instant> {
    if !child_alive { return None; }              // 已退出：退出路径接管
    match last_check {
        None => Some(now),                        // 从未检查：立即一次
        Some(t) => { let due = t + interval; Some(if due <= now { now } else { due }) }
    }
}
```

仅 `cfg(target_os = "windows")` 生效：`about_to_wait` 末尾、且本帧**尚未**安排 `WaitUntil`
时，以 1 秒间隔升级为 `WaitUntil`。Linux 路径完全不进入该分支。

- 单测 5 条：立即 / 已退出 / 等满间隔 / 已超时立即 / ≤1 次每秒。
- 未改 `Pty` trait、未改 `windows.rs`、无新依赖。
- Linux 回归：`case50` 2/2（空闲静默不变）、矩阵 8/8、workspace **188 测试**全过、
  Windows 目标 clippy 零告警。
- **Windows 空闲开销上限**：最多 1 次唤醒/秒（工程判断，置信度中；Windows 上未实测）。
  推翻条件：若 Windows 实测读端能可靠 EOF，可整段删除该分支。

## 6. Windows 3S 交接

1. **shell 退出**：在 Windows 上执行 `exit` 与直接结束 shell 进程，确认窗口在 ~1 秒内关闭
   （t3 的低频存活检查生效；Linux 走 EOF 唤醒，不涉及）。
2. **空闲开销**：用 `APP_PERF_FRAMES=1` 观察空闲 12 秒内的统计行与 CPU 时间——预期每秒最多
   1 次唤醒（统计行仍会每 5 秒出现一次，这是 t3 的直接后果，与 Linux 的"完全静默"不同）。
3. **本次修的维护块缺陷在 Windows 同样存在过**（GPU/WARP 下拖选自动滚动、OSC 标题），
   请在 WARP 上复跑 `timer_dep_matrix` 的 ①⑤。

## 7. 未解决问题

1. **Windows 的 t3 分支未在 Windows 实测**（本机只能编译与单测纯函数）。
2. **`case21` 在 GPU 后端不达标**（23 行 vs >28）：属用例时间标定问题，未改用例阈值。
3. **`xdotool click --repeat N` 不被识别为双击/三击**：app 依赖两次独立 ButtonPress（<300ms 同格），
   `--repeat` 在同一进程内连续发送不被识别；已在矩阵脚本注释中记录，改用两次独立 `click`。

## 8. 手动清单（≤3）

1. Windows 上验证 `exit` 与结束 shell 后窗口 ~1 秒内关闭。
2. Windows 上用 WARP 复跑拖选自动滚动与 OSC 标题两项（t2c 修的缺陷在 Windows 上也存在过）。
3. 观察 Windows 空闲时的唤醒频率是否与"≤1 次/秒"相符（若发现更高，请回报实测值）。
