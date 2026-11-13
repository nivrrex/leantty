# PHASE3E-WIN：阶段 4C Windows 复验交付

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4c-done`；最小化/恢复守卫真实复验；反色复验；用新观测拆分启动缺口；ConPTY 吞吐外部参照与粘贴停滞复测。未做 GPU，未改 Pty/Renderer trait，未加依赖，**未改共用代码**。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并

| 项 | 值（均取自 `git ls-remote origin` 实际输出） |
|---|---|
| tag 对象 | `refs/tags/phase4c-done` = **8e7abfe4ecc38c7b41ac6ebde170ec8a6ce61a84** |
| 解引用 commit（`refs/tags/phase4c-done^{}`） | ls-remote **无此行** —— 该 tag 为**轻量 tag**（`git cat-file -t phase4c-done` = `commit`），不存在独立 tag 对象，tag 直接指向上述 commit |
| 合并结果 | 快进：4a2d83d → **8e7abfe**，0 冲突 |
| Windows 专属文件 | hash 全部未变：`crates/pty/src/windows.rs` 96416daa、`crates/pty/tests/pty_windows.rs` c3bb4b20、`docs/WIN-NOTES.md` 265502ce、`.cargo/config.toml` 311a5385；合并 diff 中 `scripts/windows/`、`vt-verify-win/`、`crates/pty/` 前缀 0 个文件 |

## 2. t2 构建与依赖

| 项 | 结果 |
|---|---|
| debug 构建 | rc=0，32.1 s |
| release 构建 | rc=0，15.9 s，**零告警** |
| clippy dev `--all-targets -D warnings` | rc=0（vt-verify-win/logs/3e-clippy-dev.log） |
| clippy release `--all-targets -D warnings` | rc=0（vt-verify-win/logs/3e-clippy-rel.log） |
| cargo test --workspace | rc=0，35 个测试目标全绿，0 FAILED |
| 依赖树 | `cargo tree --target x86_64-pc-windows-gnu -e normal --workspace --prefix none`：134 行、去重 80 个 name@version；`grep -cE 'wgpu|pollster'` = **0** |

## 3. t3 最小化/恢复（4C 的 P1 守卫）

用 debug 构建（`APP_DUMP_SNAPSHOT` 仅 debug 可用，已在报告中注明）；release 的启动数据见 §5。

| 轮次 | 正常时 dump 头 | 最小化期间 | 恢复后 | 基线噪声（连续两帧） | 恢复前/后逐像素 |
|---|---|---|---|---|---|
| 1 | cols=120 rows=30 | **cols=120 rows=30** | cols=120 rows=30 | 0 | **0**（暗像素 7013→7013） |
| 2 | 120×30 | **120×30** | 120×30 | 0 | **0**（6420→6420） |
| 3 | 120×30 | **120×30** | 120×30 | 0 | **0**（6420→6420） |
| 最大化→还原 | 120×30 | 最大化期间 240×63（正常放大网格） | 120×30 | — | **0**（1084→1084） |

- 已验证事实：3D 复现的"最小化收到 1×1、内容被裁"**已修复**——最小化全过程中 dump 头保持 120×30，恢复后画面逐像素一致（基线噪声 0，故 0 差异有意义）。
- 内容含汉字与反色行（用例命令写入 `汉字ABC mixed text` 与 `ESC[7mINVERSE LINE ESC[0m`）。

## 4. t4 反色复验

口径与 Linux case34 一致：10 个反色空格 = 10 格 × 128 px = 1280 px，阈值 **≥768**（case34 用纯白像素计数）。Windows 侧配色改为深色主题（背景 #000000 / 前景 #ffffff）以便同口径计数。

| 用例 | 观测 | 判定 |
|---|---|---|
| A 默认色 `ESC[7m` + 10 空格 | 高亮像素（RGB 和 ≥450）**2768** | ≥768 ✔ |
| B 彩色 `ESC[7m ESC[31m` + 10 空格 | 红色像素（R≥150,G≤140,B≤140）**1280**（正好 10 格） | ≥768 ✔ |
| C 选区反色（拖选 17 列） | 选区前后逐像素差 **2176** | >0 ✔ |

- 已验证事实（记录，不下结论）：本机 CPU 渲染下"白色"实际像素为 **(197,200,198)**、反色红块为 **(204,102,102)**，不是 #ffffff/#ff0000；纯白像素计数因此为 87（若按 #ffffff 严格匹配会误判为失败）。换算比例约 0.77，原因未核实。
- 因此本报告的高亮判据用"亮度阈值"，与 case34 的"纯白计数"在**阈值数量级**上同口径（670 以上的像素量），差异已写明。

## 5. t5 启动拆分（release，5 次，ms）

新打点来自 4C：`renderer_ready_ms`（渲染器+字形缓存初始化完成）、`pre_first_submit_ms`（首帧提交前一刻）。

| 配置 | config | window | surface | pty_spawn | fontdb | 字体组 | renderer_ready | pre_first_submit | first_frame | bg_fonts |
|---|---|---|---|---|---|---|---|---|---|---|
| 默认（中位） | 26 | 86 | 4 | 10 | 46 | 138 | **669** | **740** | **764** | 903 |
| 默认（5 次 first_frame） | — | — | — | — | — | — | 669/585/677/606/690 | 740/651/748/704/760 | 764/672/772/719/794 | — |
| examples/windows.toml（中位） | 1 | 77 | 1 | 105 | 47 | 135 | 651 | 811 | 846 | 1438 |

缺口拆分（默认 5 次中位）：

| 段 | 时长 | 说明 |
|---|---|---|
| 已有点位合计 | ≈ 310 | config 26 + window 86 + surface 4 + spawn 10 + fontdb 46 + 字体组 138 |
| **renderer_ready 之前的其余部分** | **≈ 359** | 位于 `create_renderer` 内部、字体加载日志之后（字形缓存/图集构建等），**无打点** |
| renderer_ready → pre_first_submit | 71 | 事件循环启动 + 首次快照 |
| pre_first_submit → first_frame | 24 | 首帧渲染 + 提交 |

- 与 3D 对照：3D 是 first_frame≈701、未覆盖≈420；本轮 first_frame 764，新打点把尾部 420 拆成 71 + 24 = 95 ms，**仍有 ≈359 ms 未覆盖（>100 ms）**，写入请求栏（请求 1）。

## 6. t6 ConPTY 吞吐外部参照（release，120×30，2 000 000 行）

| 条件 | 总时长 | feed_bytes_per_s | chan_depth_max | render 中位 | present 中位 |
|---|---|---|---|---|---|
| 本 app / 系统 conhost | 330 s（3D 三次 323.6/333.7/330.1）；本轮 2M 运行即 done2m 记录 22:22 完成 | **442 346 / 469 556 / 467 945** | **1** | 3–4 ms | 1–2 ms |
| 本 app / WezTerm conpty（sideload） | 3D 一次 229 s；本轮一次 **467 s**（同条件两次差 2 倍） | **370 925 / 386 577** | **1** | 4 ms | 1 ms |
| **WezTerm 终端自身**（`wezterm-gui start -- cmd /c tp2m.cmd`，同文件、同计时口径） | **>900 s 未完成**（进程在我设定的 900 s 上限时仍在跑，随后按 PID 结束） | 取不到（无 app 侧观测） | — | — | — |

- 只写数据：WezTerm 终端自身处理同一 2M 行输出**比本 app 更慢**（>900 s vs 330 s），说明该场景下 ConPTY 输出路径本身是主要瓶颈，本 app 侧 feed 与渲染均有余量（chan_depth_max 恒为 1，render 3–4 ms，≈76–90 fps）。
- 已验证事实：`chan_depth_max` 在两种 ConPTY 下都只有 1，即读线程→channel→vt.feed 从未积压。
- 待验证：WezTerm conpty 条件下本 app 的两次吞吐差 2 倍（229 s vs 467 s），原因未定位（可能与同期 WezTerm 自身实例/整机负载有关）。

## 7. t7 粘贴停滞复测（release）

| 条件 | 结果 |
|---|---|
| 系统 conhost，1 MB → 原始读端 | **1048576 字节，7.77 s**（首次尝试因粘贴未注入而 0 字节，重试成功；属注入抖动） |
| WezTerm conpty，1 MB → 原始读端 | **本轮 2 次都完成**：第 1 次 t=5 s 已 884 986 字节、t=15 s 达 1 048 576；第 2 次 t=5 s 958 714、t=15 s 1 048 576。**3D 记录的"停在约 45 万字节"未复现** |

- 停滞期间是否仍在收字节：3D 的停滞样本里 `BYTES` 在 300 s 内不变（未增长）；本轮无停滞，故无该现象可测。
- feed_bytes_per_s / chan_depth_max：**该配置下取不到** —— 原始读端（pp.exe）不回显，app 无屏幕变化即不产生帧，`APP_PERF_FRAMES` 行不会打印。已改用输出路径的 2M 运行给出同口径指标（§6）。
- 待验证：3D 的停滞（3/3）与本轮正常（0/2）之间的差异条件未定位。

## 8. 未解决问题

1. 启动仍有 ≈359 ms 无打点（renderer 内部、字体日志之后），>100 ms，已入请求栏。
2. WezTerm conpty 下 2M 行吞吐两次差 2 倍（229 s / 467 s），未定位。
3. 3D 的粘贴停滞（~450 KB）本轮未复现（0/2），条件差异未定位。
4. 反色块渲染出的白色为 (197,200,198) 而非 #ffffff，原因未核实（未影响可见性判定）。

## 9. 对共享代码的请求

1. **（观测增强）** `create_renderer` 内部在字体加载完成到返回之间仍有约 359 ms（Windows release，5 次中位）无打点；建议在字体加载完成之后、字形缓存/图集构建之后各加一个 `[perf]` 点（不改行为）。加上现有的 renderer_ready / pre_first_submit，即可把 764 ms 首帧完全拆开。
2. **（现象记录，待验证）** WezTerm conpty 下 2M 行吞吐两次相差 2 倍（229 s / 467 s），同期 WezTerm 终端自身 >900 s 未完成；建议在 Windows 侧保留"外部终端参照"这一测量项，暂不据此改代码。

## 10. 交付物与证据

- 脚本：`vt-verify-win/{run_t3_minimize,run_t4_inverse,run_paste_sample,run_paste2,run_throughput2,run_perf3c}.ps1`。
- 配置：`scripts/windows/e2e/cfg/inverse-dark.toml`（深色主题，反色对照用）。
- 日志/截图：`vt-verify-win/logs/{3e-clippy-dev,3e-clippy-rel,3e-test,3e-tree,3e-perf}.log`；`vt-verify-win/logs/phase2b-win/{t3-*,t4-*,paste-*,tp2-*}.{png,err,dump}`。
- 未入库：`vt-verify-win/perfdata/`（含 130 MB 测试文件）、任何 conpty.dll/OpenConsole.exe。

## 10.5 推送与打 tag 核对（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `fb86374ab7847fe0b4cde5f3336c954170785b32` |
| `refs/tags/phase3e-win-done`（**tag 对象**） | `d5ed02059d38796fa20b996fc28b673e44be4527` |
| `refs/tags/phase3e-win-done^{}`（解引用 commit） | `fb86374ab7847fe0b4cde5f3336c954170785b32` |
| `refs/tags/phase4c-done`（本次合并的 tag，轻量） | `8e7abfe4ecc38c7b41ac6ebde170ec8a6ce61a84`（无 `^{}` 行） |

- `git push origin win`：`4a2d83d..fb86374  win -> win`；tag 为新建后 push（`* [new tag] phase3e-win-done`），未使用 `--force`。

## 11. 手动清单（≤3）

1. 在装有 Windows Terminal 的机器上重复 §6 的"终端自身 2M 行 type"对照（本机只有 WezTerm 可作外部参照）。
2. 用真实输入法确认反色选区在中文行上的可读性（本机只做了像素统计）。
3. 若关注启动耗时，可在 `renderer_ready` 之前补打点后复测（本机数据见 §5）。
