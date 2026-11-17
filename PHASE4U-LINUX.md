# PHASE4U-LINUX：合并检查 / 自动滚动速率核查 / 一次性 rustfmt / 最终验证

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 Windows | **无新提交**（win 头仍是 `1791c76`，4T 已合并），跳过合并，tag 打在当前 HEAD |
| t2 自动滚动速率 | 三后端中位差 ≤2.7%（判据 25%）→ **速率与帧耗时无关，不改代码**（tag `phase4u-t2`） |
| t3 rustfmt | 45 文件格式化（两个 Windows 专属文件按约定排除），无行为变化，门禁新增第 9 步（tag `phase4u-t3`） |
| t4 最终验证 | GPU 30 分钟通过（+3.2%）；**CPU 30 分钟在 21.6 分钟提前退出**（无 panic，shell 随 Ctrl-C 退出，见 §4）|

## 1. t1 合并检查（ls-remote 实测）

```
8132ade1984084f8f95186529293d4d2f31bf06c	refs/heads/main（本阶段起点）
1791c76d470adf82c37c35686f0f9ebc478c763b	refs/heads/win
85b842bb6e62fa43283b8b3f5a7c4d003a7dbfd0	refs/tags/phase3q-win-done
cc43cf03529fd123a8a26cb964c4640f71edd780	refs/tags/phase3q-win-done^{}
```

`refs/heads/win` 仍是 `1791c76`（3Q 的补记 hash 提交，4T 已合并），**自该提交起没有任何新的
Windows 提交**（也没有 `phase3r/3s` 系列 tag）→ 按任务说明"无新提交则写明并跳过合并"；
`phase4u-t1` 打在当前 HEAD（`8132ade`，其内容与 4T 交付一致）。Windows 侧提到的"revert 格式调整"
提交在本轮实测中**不存在**，`crates/` 相对 main 零差异。

## 2. t2 拖选自动滚动速率核查

### 2.1 审计（推进逻辑）

`crates/app/src/selection.rs::auto_scroll_step(offset, dir, sb_rows, viewport_rows)`：

- **每次调用推进 1 行**（`offset ± 1`，clamp 到 `sb_rows`），并把选区 focus 设为"视口最后可见行
  （向下）或顶行（向上）"；
- 调用点在 `about_to_wait` 的维护块里，**每次迭代调用一次**；块尾 `request_redraw()` 自驱动下一次迭代；
- **不使用任何时间差**（`Instant` 只用于双击判定）→ 理论上是"帧驱动"，速率 = 迭代率。

### 2.2 量化（release，:99，`scripts/e2e/autoscroll_rate.sh`，各 3 次）

方法：输出 2000 行 → 滚到顶部 → 窗口内按下左键、指针移到窗口下缘之外并**静止 3 秒** → 松开；
用滚动条拇指位置换算偏移（`offset ≈ ty/(ph−th)·sb_rows`，与 `scrollbar_geometry` 互逆）。

| 后端 | 3 秒推进行数（3 次） | 中位 | 相对 cpu | 该轮最大帧间隔 |
|---|---|---|---|---|
| cpu | 1124 / 1134 / 1124 | **1124**（≈375 行/秒） | — | 106ms |
| gpu（NVIDIA） | 605 / 1094 / 1099 | **1094** | **−2.7%** | 106–107ms |
| lavapipe | 1104 / 1114 / 1109 | **1109** | **−1.3%** | 106–107ms |

### 2.3 判定

中位差 **≤2.7%**，远小于 25% 判据 → **c) 不成立：推进速率与后端/帧耗时无关**，
按任务 f) **不修改代码**。机理：维护块在渲染块**之前**，`request_redraw()` 本身极便宜，迭代率由
事件循环与 `RENDER_MIN_INTERVAL`(8ms) 下限主导，而不是被渲染耗时主导（三个后端的帧间隔统计也都是
106–107ms 的最大值，中位差异很小）。

**异常点**：gpu 有一次 605 行（−46%），与该后端另两次 1094/1099 差得远；未能稳定复现，
登记为待验证假设（可能是该轮首帧/着色器编译占用了迭代时间）。推翻条件：若 GPU 上稳定出现
≥25% 差距，则应改为按时间推进（速率取 cpu 的 375 行/秒）。

### 2.4 case21 复核（阈值未改）

用 case21 的同一序列受控复现（`seq -w 1 100`、8 次上滚、拖到下缘悬停 1.2s、右移 0.4s、松开）：
**cpu 与 gpu 均得到 70 行、357 字节、无 scrollback 截断警告** —— run_all 里出现过的 CPU 41 /
GPU 23 的差异**未复现**，说明既有实现的速率本身一致；此前的 GPU 不达标更可能是环节时序/干扰。
用例与阈值均未修改。

## 3. t3 一次性 rustfmt

### 3.1 初始偏差

`cargo fmt --all --check` 初始命中 **47 个文件**（含两个 Windows 专属文件）。

### 3.2 处理

- 执行 `cargo fmt --all`；
- 按任务约定 `git checkout -- crates/pty/src/windows.rs crates/pty/tests/pty_windows.rs`
  （Windows 专属文件，Linux 侧不得修改，留给 Windows 侧）；
- 结果 **45 个文件**变更（+1424/−671），`vendor/` 未被动（`git status vendor/` 为空）。

### 3.3 变更性质（抽样 + 全量归一化）

- 抽样 3 个文件确认只有格式改动：
  - `crates/app/tests/reader.rs`：`use` 重排（`{reader_body, Error, Wake}` → `{Error, Wake, reader_body}`）、
    单表达式闭包改为块体；
  - `crates/app/src/clipboard.rs`：去空白后**字符多重集完全相同** → 仅 `use` 排序；
  - `crates/render/src/gpu/mod.rs`：多行列表补尾逗号、换行重排。
- 全量 45 文件做"去空白后字符多重集"比对：差异仅为**新增尾逗号**与**包裹花括号**
  （`{expr}` → `{ expr }` 之类），无标识符/字面量/运算符变化。`git diff -w` 仍有行是因为
  rustfmt 会重排 `use` 与重折行（`-w` 不忽略顺序与断行）。

### 3.4 无行为变化证据

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **188 passed / 0 failed** |
| `frame_bytes_identical` 黄金哈希 | **507485fb7945fb8f**（与格式化前一致，指纹不变） |
| `gpu_parity` | **10/10** |
| `cargo check --target x86_64-pc-windows-gnu --workspace --tests` | 0 error / 0 warning |
| e2e `run_all.sh` | **108/108**（GPU 轮 10/10） |

### 3.5 门禁新增第 9 步

`scripts/gates.sh` 新增 rustfmt 检查：解析 `cargo fmt --all --check` 的 `Diff in <路径>` 行，
**排除** `crates/pty/src/windows.rs` 与 `crates/pty/tests/pty_windows.rs`，其余任一文件有偏差即失败
（当前实测只剩这两个文件有偏差）。脚本内注释说明了排除理由。

## 4. t4 最终验证

### 4.1 门禁

`scripts/gates.sh` **PASS**（9 步全过）。

### 4.2 长跑（30 分钟/后端，release，:99）

| 后端 | 结果 | RSS | fd/线程 | panic | SIGTERM | CPU 占用 |
|---|---|---|---|---|---|---|
| gpu | **跑满 30 分钟** | 271384→280128KB（**+3.2%**） | 54 / 4 恒定 | 无 | 限时退出 | **≈66% 单核** |
| cpu | **t=1296s 提前退出**（21.6 分钟） | 退出前 ~102MB 平稳 | 13 / 4 恒定 | **无** | — | **≈67% 单核** |

- `loop_gap` 最大值：GPU **1753ms**、CPU 相同量级 —— 属**空闲等待**（4S 之后空闲不再被定时唤醒，
  长间隔是等待而非停顿；判停对应结合同窗口 CPU tick，见 `PHASE4S-LINUX.md` §2.6）。
- 帧日志：GPU 3718 条。
- **CPU 提前退出**：最后一条日志是分支 7 的 `[perf] ctrl_c_drop_paste bytes=1047905`，即 Ctrl-C 取消
  粘贴之后紧接着子 shell 退出 → app 按设计随 shell 退出而关闭；无 panic、无资源异常。
  定向复现（连续 8 轮 `sleep 20` + 1MB 粘贴 + Ctrl-C，每轮等 sleep 结束）**8/8 存活、未复现**，
  判为长跑负载的偶发交互（待验证假设：该 pty 上作业控制未生效时 Ctrl-C 会连带终止 bash）。

### 4.3 回归矩阵

| 脚本 | 结果 |
|---|---|
| `timer_dep_matrix.sh cpu` | **8/8** |
| `timer_dep_matrix.sh gpu` | **8/8** |
| `idle_repaint_matrix.sh nvidia cpu` | 9 项全部重绘、空闲 10 秒新增统计行 **0** |
| `idle_repaint_matrix.sh nvidia gpu` | 同上 |
| `idle_repaint_matrix.sh lavapipe gpu` | 同上 |

### 4.4 README「已知限制」

把既有"已知平台限制"与实测"行为限制"合并成一节，逐条标出处：

1. 平台限制四条（conhost 物化 `ESC[7m`、`OSC 104` 不到达、ConPTY 吞括号粘贴/SGR + CRLF 物化、
   Windows Ctrl-C 后已入缓冲的粘贴无法撤回），保留原有"使用系统 conhost 时仍适用"的限定。
2. **更正**原"GPU 后端仅 Linux"一条：4P 起 Windows 也可用 GPU，但 `auto` 恒为 CPU、
   `gpu` 为显式实验开关（依据 `PHASE4P-LINUX.md` §2.3、`PHASE4R-LINUX.md` §5）。
3. 新增行为限制七条：粘贴积压 12MiB 整条拒绝（4L §2/§3）、7MB 粘贴在回显开启时的回显背压
   （4H §3.5）、CPU 后端事件帧仍走整帧快照比对（4S §7）、`loop_gap` 含空闲等待（4S §2.6）、
   Windows 每秒一次存活检查（4T §5）、Windows 滚动条列疑似色彩管线差异 1.46%（4S §3，**该归因已由 3T t5 推翻，见 4W 更正**）、
   `case21` 在 GPU 后端时间标定偏紧（4U §2）、以及"只识别 4 个自身环境变量"（4R §3）。

## 5. Windows 交接

1. **格式化冲突预警**：本次 `cargo fmt --all` 会让 Windows 侧 worktree 下次合并时出现大面积格式
   冲突；约定 Windows 侧**不得再改 `crates/`**（需要格式时执行 `cargo fmt --all` 并只提交格式变更）。
   本次格式化提交为 `fde48a3`（tag `phase4u-t3`）。
2. `crates/pty/src/windows.rs` 与 `crates/pty/tests/pty_windows.rs` **未被格式化**（门禁已排除），
   请 Windows 侧自行 `cargo fmt` 后提交；届时可把这两个路径从 gates 的排除名单移除。
3. t1 确认 Windows 侧无新提交；若 Windows 侧后续有 `crates/` 改动，请先 rebase 到本次格式化提交之上。

## 6. 未解决问题

1. **CPU 30 分钟 soak 未跑满**（21.6 分钟时 shell 随 Ctrl-C 退出）；8 轮定向复现未重现，
   判为负载偶发交互，但**未定位到确定机制**（置信度中）。
2. **GPU 一次自动滚动 605 行**（其余 1094/1099）的异常未定位。
3. **`case21` 在 run_all 的 GPU 轮未覆盖**（该用例用 cpu 后端），GPU 侧时间标定问题只是绕开、未修。

## 7. 手动清单（≤3）

1. Windows 侧合并后先 `cargo fmt --all` 并提交两个 pty Windows 专属文件的格式变更。
2. 若后续在 Windows 上再次出现 Soak 类长跑中"Ctrl-C 后窗口关闭"，请记录当时前台命令与是否交互式。
3. 若 GPU 自动滚动出现明显变慢（>25%），回报帧间隔与机器型号，以便按时间推进改造。
