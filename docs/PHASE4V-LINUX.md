# PHASE4V-LINUX：修复 Windows 存活检查未唤醒（Linux 侧取证、修复与门禁）

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。本机为 Linux（Devuan 5，
DISPLAY=:99），**Windows 侧结论一律标注为待验证假设**。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 取证 | `about_to_wait` 里存活唤醒登记**只有一处、且在函数末尾**，其间 7 处提前 return 全部跳过它；新增诊断开关 `APP_FORCE_LIVENESS_POLL=1`；**在未修复代码上复现成功**（空闲 15 秒 0 条统计行）|
| t2 修复 | 收敛到唯一出口 `finish_wake` + 纯函数 `next_wake`（5 条单测）；193 测试全过、黄金哈希不变 |
| t3 门禁 | 新增 case51（三后端 1 次/秒）与 case52（≤3s 退出）；回退验证 case51 变红；无开关基线未退化 |
| t4 文档 | README 诊断开关、DECISIONS 7 条、本文件 |
| t5 收尾 | `scripts/gates.sh` 全过，tag `phase4v-t1`/`phase4v-done`，push main + tags |

## 1. t1a `about_to_wait` 的提前 return 清单（修复前，行号对应 `76e0b6a`）

| # | 位置（修复前） | 条件 | 是否跳过存活唤醒登记 |
|---|---|---|---|
| 1 | `if self.no_render { return; }` | debug 专有（禁渲染开关） | **是** |
| 2 | 帧节流 `if !due { … return; }` | 距上一帧不足自适应间隔 | **是**；`last_render` 为 `Some` 时先设了 `WaitUntil(target)`（8–100ms 内必醒），为 `None` 时**无任何唤醒** |
| 3 | `if !is_gpu && surface_guard.is_none() { return; }` | CPU 后端无 softbuffer surface | **是** |
| 4 | GPU 空闲跳过（4Q）`is_gpu && !urgent && !full_redraw && clean && 几何未变` | GPU 后端空闲 | **是**（Windows 空闲必命中此分支）|
| 5 | CPU 无脏区跳过 present | 完全静态帧（静止画面零提交） | **是** |
| 6 | `let Some(surface) = surface_guard.as_mut() else { return; }` | surface 取不到 | **是** |
| 7 | `if !force_full_present && damage.is_empty() { … return; }` | 无脏区 | **是** |

结论（已验证事实）：登记点位于函数**末尾**，7 处 return 任一命中即不登记 → 事件循环停在无限
`Wait`，`try_wait` 再也不会被调用。这解释了 Windows 3S 的"30 秒只有 1 条统计行""最小化/还原
（事件唤醒）后立即退出"。

## 2. t1c 复现（未修复代码，Linux，release，:99，cpu 后端）

诊断开关 `APP_FORCE_LIVENESS_POLL=1` + `APP_PERF_FRAMES=1`（直接 `2>file` 重定向）：

```
空闲 15s 前日志行数=59
=== 空闲 15s 期间新增日志（原文）===
(无统计行)
=== 新增行数 ===
0
```

最后一条统计为 `loops=1 wake_timer=0 wake_event=1 wake_pty=0` —— 一次 `about_to_wait` 都没发生，
与 Windows 现象一致（Windows 上 30 秒仅 1 条，本机更彻底）。同时：杀掉记录 PID 的子 shell 后
10 秒 app 仍存活（当时用 `SIGTERM`，**交互式 bash 会忽略 SIGTERM**，故该次不作为证据；改用
`SIGKILL` 后见 §4）。

## 3. t2 修复

- 唯一出口 `AppHandler::finish_wake(event_loop, timer_at)`：合并"存活检查到期"与"其它定时待办"
  （渲染节流目标、debug 钩子），取最早者设 `ControlFlow::WaitUntil`；两者皆无则 `Wait`。
- 纯函数 `next_wake(now, liveness, render, paste) -> Option<Instant>`：取最早、过期归一化为 `now`。
  单测 5 条：仅存活 / 存活+渲染（含"渲染更晚不能拖住存活检查"）/ 存活+粘贴 / 均无 / 过期归一化。
- 7 处提前 return 与函数末尾都改为经 `finish_wake`；入口保留 4S 的 `set_control_flow(Wait)`。
- 删除 `wait_scheduled` 字段（它只服务旧的末尾登记判断，统一出口后无读取者）。
- **否决项**：入口先登记存活唤醒、后续分支覆盖 —— 后续若有更晚的 `WaitUntil`（debug 钩子
  `test_paste_at`）会覆盖掉存活唤醒。

修后同一测量：空闲 15 秒得 **3 条**统计行，逐行 `loops=5 wake_timer=5`（= 1 次/秒）。
`SIGKILL` 直接子进程后 **146ms** 退出。

行为不变：workspace **193 测试全过**；`frame_bytes_identical` 黄金哈希 **507485fb7945fb8f**
（字体指纹 `DejaVuSansMono.ttf:343140:c69b:8x16` 匹配，非 SKIP）。

## 4. t3 门禁

### 4.1 新增用例（`scripts/e2e/run_all.sh`）

- **case51**：`APP_FORCE_LIVENESS_POLL=1` + `APP_PERF_FRAMES=1`（`2>file` 直接重定向），三个后端
  （cpu / gpu NVIDIA / lavapipe，后者用 `APP_GPU_ALLOW_SOFTWARE=1` + `VK_ICD_FILENAMES=…lvp…`）
  各空闲 15 秒；统计行每 5 秒一条，断言**稳态**两行的 `loops` 与 `wake_timer` 都落在 **4–6**
  （依据：5 秒窗口 / 1 秒间隔 = 5，允许 ±20% 的边界抖动；窗口首行可能横跨进入前，故只看最后两行）。
- **case52**：同一开关下 `SIGKILL` app 的直接子进程（只按 `pgrep -P $APP_PID` 记录的 PID），
  断言 app 在 **≤3000ms** 内退出（用 `ps -o stat=` 判僵尸/消失，避免 `kill -0` 对僵尸恒真）。

首轮实测（修复版）：

```
  [c51 cpu] 空闲 15s：统计行=3 loops=[1 13 5 5] wake_timer=[0 11 5 5] 稳态越界=0
  [c51 gpu] 空闲 15s：统计行=3 loops=[1 8 5 5] wake_timer=[0 6 5 5] 稳态越界=0
  [c51 lavapipe] 空闲 15s：统计行=3 loops=[1 9 5 5] wake_timer=[0 7 5 5] 稳态越界=0
  [c52 cpu] 杀子 shell 后 35ms 退出（判据 ≤3000ms）
  [c52 gpu] 杀子 shell 后 154ms 退出
  [c52 lavapipe] 杀子 shell 后 157ms 退出
```

（首轮 case51 因判据把"进入窗口前的那一行"也算进去而误判 FAIL，已改为先按内容过滤再按行号跳过，
并只断言稳态两行；上述数值即修正后判据下的取值。）

### 4.2 回退验证（临时补丁，未提交）

把 `liveness_deadline` 强制返回 `None`（等价于修复前"存活唤醒从不登记"）后重建：

```
===== 回退后 case51 等价测量
空闲 15s 期间新增日志：
(无统计行)
新增行数=0                      ← 红
===== 回退后 case52 等价测量
结果：app 在 161ms 内退出        ← 仍绿（见下）
```

**case52 在 Linux 上不具备区分力**（已验证事实）：杀会话首进程会让 pty 必然挂断（内核在会话
首进程退出时发挂断，即使其它进程仍持有从端），读线程必被唤醒 → 与存活轮询无关。尝试让背景进程
继续持有 pty（`sh -c "sleep 25 & exec sleep 25"`）同样 154ms 即退出，无法规避。故**回归检测由
case51 承担**，case52 只证"退出路径 ≤3s"。

### 4.3 无开关基线（t3d）

- `case50`（不带诊断开关，空闲 12s）：**2/2 通过**，两后端均"新增统计行 0、CPU tick 0、ctx 增量 0"。
- `idle_repaint_matrix.sh nvidia cpu`：空闲 10 秒新增统计行 **0**（与 4R/4S 基线一致）。
- 即：诊断开关关闭时 Linux 仍为纯事件驱动、空闲零唤醒。

## 5. t4 文档

- README「诊断输出」新增 `APP_FORCE_LIVENESS_POLL=1`（release 可用，仅诊断，会带来每秒一次空闲唤醒），
  并在"本项目只识别以下变量"清单中登记。
- DECISIONS 新增 7 条（修复决定与否决项、语义、行为不变、门禁、case52 限制、无开关基线、Windows 待验证）。

## 6. 未解决 / 待验证

1. **Windows 侧未实测**（待验证假设，置信度中高）：根因与本机同一处，改动应同样生效；但需 Windows
   侧用本 tag 复跑"子 shell 退出后窗口关闭"。推翻条件：仍 >3s 则存在第二个唤醒缺失点。
2. case51 的 1 秒间隔与 4–6 判据是基于 Linux 实测；Windows 上定时器精度较差时可能需要放宽（未验证）。
3. 每 5 秒一条统计行依赖 `APP_PERF_FRAMES=1`，生产运行下该分支不输出，不影响用户。
