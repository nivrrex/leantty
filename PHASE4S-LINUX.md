# PHASE4S-LINUX：合并 3Q / 空闲定时唤醒根因治理 / 滚动条对拍 / README 更正

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3q-win-done` | 已合并 `cc43cf0`（tag `phase4s-t1`），共用代码零改动 |
| t2 空闲定时唤醒 | **根因找到并修复**：cpu 空闲 249 loops/s（100% 定时）→ **0**（tag `phase4s-t2`） |
| t3 滚动条对拍 | 新增 2 例（底部/中段），NVIDIA 与 lavapipe 列内差异 **0/4608**（tag `phase4s-t3`） |
| t4 README | ConPTY 代价改为 +105ms、补 GPU 实测事实、强调直接句柄重定向（tag `phase4s-t4`） |
| t5 DECISIONS | 4R 的 auto 翻转条件已更正（tag `phase4s-t5`） |

## 1. t1 合并证据（ls-remote 实测）

```
451a30375d3d602774cc656def8773a641558c20	refs/heads/main（合并前）
1791c76d470adf82c37c35686f0f9ebc478c763b	refs/heads/win
85b842bb6e62fa43283b8b3f5a7c4d003a7dbfd0	refs/tags/phase3q-win-done
cc43cf03529fd123a8a26cb964c4640f71edd780	refs/tags/phase3q-win-done^{}
```

- 附注标签，解引用 = `cc43cf0`；`refs/heads/win` = `1791c76`，多一个仅改 `PHASE3Q-WIN.md` 的补记提交。
- Windows 侧自有提交（`cc43cf0` / `ebd7d40`）**未触及 `crates/`**（逐提交 `git diff-tree` 为 0）；
  改动在 `scripts/windows/tonight/`、`vt-verify-win/`、文档。合并后 `crates/` 零差异。

## 2. t2 空闲定时唤醒：根因、修复、门禁

### 2.1 量化（修复前，release，:99，空闲 20 秒）

| 后端 | loops（5 秒窗口） | loops/s | wake_timer | wake_event | wake_pty | CPU tick/20s | voluntary ctx |
|---|---|---|---|---|---|---|---|
| cpu | **1246** | **249** | **1246（100%）** | 0 | 0 | 267（≈13% 单核） | 2490 |
| gpu | 178 | 36 | 178（100%） | 0 | 0 | 36 | 383 |

新增观测：每 5 秒统计行追加 `loops/wake_timer/wake_event/wake_pty`（`about_to_wait` 入口按
"本轮是否由窗口事件 / PTY 唤醒"分类，其余计为定时唤醒）。

### 2.2 取证：空闲时谁在设置 WaitUntil

| 站点 | 条件 | 空闲时是否生效 |
|---|---|---|
| `about_to_wait` 的 `!due` 分支（尾沿/合并） | 上一次渲染距今 < 帧间隔 | 需要（有未渲染变化时） |
| debug 测试钩子（`APP_TEST_PASTE_AT_MS`、`APP_TEST_PREEDIT_CLEAR_MS`） | debug 专有 | release 不编译 |
| **控制流未重置（根因）** | 无 | **是**：上一轮 `WaitUntil` 的时刻已过，winit 立即返回 → 再跑一轮 → 再渲染 |

即：`about_to_wait` 在"到期渲染"后直接返回、**没有重新设置控制流**，winit 保留上一轮的
`WaitUntil(过去时刻)` → 立即唤醒 → 再次 `due` → 再渲染 → 无限热循环。

"何时需要定时唤醒"的清单（修复后仍然保留的）：① 尾沿帧（帧间隔未到而仍有未渲染变化，
`case28` 依赖）；② 粘贴预算化排空的自驱动（`case25/32/39/47`）；③ debug 测试钩子定时器；
④ 写线程/退出超时（在各自线程内，与事件循环无关）。

### 2.3 修复

`about_to_wait` 入口先 `set_control_flow(ControlFlow::Wait)`（默认无限等待），随后各分支只在
确实有待办工作时才升级为 `WaitUntil`（尾沿 `!due` 分支、debug 钩子）。帧合并与自适应间隔的
数值/算法未改；4Q 的 GPU 空闲跳过**保留**为第二道防线。

### 2.4 效果与门禁

| 后端 | 修复后空闲 12 秒 | CPU tick | voluntary ctx |
|---|---|---|---|
| cpu | **无新增统计行（loops=0）** | **0** | **0** |
| gpu | **无新增统计行（loops=0）** | **0** | **0** |

`case50`（新增）：输出停止并收尾后 12 秒内不得出现新的 5 秒统计行、tick ≤24、ctx <150。

| 版本 | 结果 |
|---|---|
| 修复后 | **2/2 PASS**（新增统计行 0、tick 0、ctx 0） |
| 回退修复 | **0/2**（cpu 3 行 / 160 tick / 1494 ctx；gpu 3 行 / 45 tick / 455 ctx）→ 证伪成立 |

`case49`（GPU 空闲）判据同步更新：4S 之后空闲时不再产生 5 秒统计行，改为"空闲/缩放收尾后
10 秒内无新增统计行"（比"帧数 ≤3"更强），修复后 2/2 PASS。

### 2.5 附带发现与修复：日志刷屏导致的真停顿

`[selection] scrollback 截断` 警告在"拖选 + 持续输出"下**每帧**打印：6 分钟 soak 实测
**175,403 行**（约 490 行/秒），stderr 写入本身把主线程拖出 **1708ms** 的停顿（`loop_gap`）。
已限速为同一警告每秒最多一条（语义不变），同负载再跑 6 分钟降到 **587 行**。

### 2.6 关于 soak 里 ≥100ms 的 `loop_gap`（不得忽略项）

修复后 `loop_gap` 会把**空闲等待**也计入（事件循环正确地进入 Wait）。用 5 秒窗口同时采 CPU
tick 验证：

| 窗口 | loop_gap_ms | 该窗口 CPU tick（10ms/tick） | 解释 |
|---|---|---|---|
| 1–3 | 0 | 2 / 0 / 0 | 空闲 |
| 4 | **18747** | **109（1.09s）** | 主线程在等待（shell 在 `sleep 20`），不是阻塞 |
| 5–6 | 11 / 8 | 409 / 393 | shell 消费刚粘贴的 780KB，属真实工作量 |

soak 6 分钟的最大值 **1708/1709ms →（限速后）2001ms** 同属空闲等待（same window CPU 占用很低）。

### 2.7 浸泡（release，6 分钟/后端）

| 后端 | RSS 预热末→峰值 | 判据 | fd | 线程 | panic | SIGTERM |
|---|---|---|---|---|---|---|
| cpu | 97876→99280KB（**+1.4%**） | ≤10% ✅ | 恒定 | 恒定 | 无 | 限时退出 |
| gpu | 271972→272968KB（**+0.4%**） | ≤10% ✅ | 恒定 | 恒定 | 无 | 限时退出 |

### 2.8 重绘矩阵（扩展为接受后端参数）

`scripts/e2e/idle_repaint_matrix.sh <nvidia|lavapipe> <cpu|gpu>`：

| 项 | nvidia/cpu | nvidia/gpu | lavapipe/gpu |
|---|---|---|---|
| 1a 选区高亮 | 18837 | 18837 | 18837 |
| 1b 清除选区 | 19110 | 19110 | 19110 |
| 3a 滚轮上滚 | 7588 | 7588 | 7588 |
| 3b 滚回底部 | 7861 | 7861 | 7861 |
| 6a OSC11 改背景 | 4460 | 4460 | 4460 |
| 6b OSC111 复位 | 5118 | 5118 | 5118 |
| 7a 光标隐藏 | 5474 | 5474 | 5474 |
| 7b 光标显示+下划线 | 5901 | 5901 | 5901 |
| 8 窗口缩放 | 185826 | 185826 | 185826 |
| 空闲 10 秒新增统计行 | 0 | 0 | 0 |

全部 1 秒内重绘，且修复后空闲完全静默（0 新增统计行）。

## 3. t3 滚动条对拍

`gpu_parity` 新增两例（`rows_sb=2000`，视口在底部 / 居中），只比较右缘
`SCROLLBAR_WIDTH`(12px) 列内像素（阈值沿用背景区：最大通道差 ≤1、差异占比 ≤0.5%）：

| 适配器 | 视口 | 列内差异 | 最大通道差 | 列外差异 |
|---|---|---|---|---|
| NVIDIA GT 710 | 底部 | **0 / 4608（0.000%）** | 0 | 0.000% |
| NVIDIA GT 710 | 中段 | **0 / 4608** | 0 | 0.000% |
| lavapipe | 底部 / 中段 | **0 / 4608** | 0 | 0.000% |

> **更正（阶段 4W t4，2026-11-17）**：本条被 Windows 3T t5 推翻——960 宽实测差异 1.0304%，
> 其中滚动条列仅 9 px（0.156%），主要差异在字形光栅；见 `PHASE3T-WIN.md` t5 与
> `docs/DECISIONS.md` 的 4W 更正条目。以下原文保留备查。

**对 Windows 3Q「1.46% 像素差 >32」的解释**（工程判断，置信度高，**已被推翻**）：滚动条列正好占画面宽度
`12/800 = 1.500%`，与 1.46% 几乎相等 → 该差异几乎全部集中在滚动条列。滚动条的轨道色是
"前景/背景逐通道平均"、拇指是纯前景色，对 1 LSB 级的色彩/混合差异最敏感。Linux 上两个适配器
列内差异都是 0，说明这是 Windows 侧（WARP/DX12 或主机端色彩管线）的差异，不是两后端逻辑不一致。
推翻条件：若 Windows 上按同法统计出列外也显著（>0.3%），则该解释不成立，应改查字形混合与 sRGB。

## 4. t4 README 改动清单

1. **ConPTY 启动代价**：改为"约 **+105ms**（419→524ms，直接句柄重定向，n=10，`PHASE3M-WIN.md`
   §4.1）"；删除旧的"+110ms / 首帧 1.6–1.9 秒"绝对值，并说明那批数据来自 PowerShell
   `Start-Process -RedirectStandardError`（每次写约 6.8ms，绝对值抬高约 4 倍，只能横向比较
   ——`PHASE3L-WIN.md` §2.1、`PHASE3M-WIN.md` §4.2）。
2. **Windows GPU 后端**：补入 2026-11-15 真实 GPU 实测（Ryzen 5 5600U 核显、Vulkan 可用、
   GPU/CPU 共有文字行最大差 1 LSB、首帧 GPU 325–356ms 对 CPU 359ms、浸泡稳定）；明确
   "DX12 尚未在真实硬件验证，只在 WARP 上验证"；`auto` 恒为 CPU、`gpu` 为显式实验开关；
   诊断开关（`WGPU_BACKEND`/`WGPU_ADAPTER_NAME`/`APP_GPU_ALLOW_SOFTWARE`/`APP_PERF_FRAMES`）
   与"wgpu 自身 `WGPU_*` 在本构建不生效"保留。
3. **诊断输出**：强调采集 `[perf]` 用**直接句柄重定向**，不要用 PowerShell `Start-Process`。

## 5. t5 DECISIONS

- t2 的根因结论与修复前后数据（loops/s、CPU tick、ctx）已登记（见 §2）。
- 4R 的"Windows auto 三条翻转条件"用**删除线**保留，追加 2026-11-16 更正：因没有可持续使用的
  真实 GPU 环境，条件 (3) 无法完成验证 → 新定性为"Windows `auto` **恒为 CPU**，`gpu` 为显式
  实验开关；如日后获得真实 GPU 反馈再重新评估（以 2026-11-15 的 Ryzen 5 5600U 结果为基线）"。

## 6. Windows 3R 交接

1. **空闲唤醒**：Windows 上按 `case50` 的口径验证——输出停止收尾后 12 秒内不得出现新的 5 秒
   统计行，且 `Get-Process` 的 CPU 时间增量应≈0（本机 Linux 实测两后端均为 0）。
2. **滚动条列**：在 WARP 与真实 GPU 上各做一次"CPU vs GPU 同内容"截图对比，统计差异像素在
   滚动条列内外的分布；若列外也显著（>0.3%），`PHASE4S-LINUX.md` §3 的解释需要修正。
3. **日志刷屏**：Windows 上若仍见大量 `[selection] scrollback 截断`（应已限速为每秒一条），
   请回报实际频率。
4. **诊断采集**：一律用直接句柄重定向（bash `2>file` 或 `cmd /c "app.exe 2>file"`），
   不要用 `Start-Process -RedirectStandardError`。

## 7. 未解决问题

1. **CPU 后端空闲仍"渲染但不提交"**：修复后空闲 CPU 已为 0 tick（因为不再被唤醒），但只要有
   事件（例如光标位置抖动）仍会走完整帧的快照+比对；4Q 的 GPU 空闲跳过是第二道防线，CPU 未加。
2. **`loop_gap` 语义变了**：修复后它包含空闲等待，不再能单独作为"停顿"指标；判定停顿应结合
   同窗口 CPU tick（本轮给出方法）。
3. **Windows 侧全部结论未复测**（本阶段所有数据来自 Linux：NVIDIA + lavapipe）。

## 8. 手动清单（≤3）

1. Windows 上按 §6.1 验证空闲不再被定时唤醒（统计行不增加 + CPU 时间≈0）。
2. Windows 上按 §6.2 复核滚动条列内/外的差异占比（验证 1.46% 的归因）。
3. 若观察到 `[selection] scrollback 截断` 仍频繁刷屏，请回报频率（已限速但未在 Windows 验证）。
