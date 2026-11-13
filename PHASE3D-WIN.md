# PHASE3D-WIN：阶段 4B-fix3 Windows 复验交付

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4b-fix3`；验证 Windows 零 wgpu 与 CPU 渲染修复；release 吞吐重测；启动打点。未做 GPU，未改 Pty/Renderer trait，未加依赖。
- `git ls-remote origin refs/tags/phase4b-fix3` = **b7aad4e3f0092220db41a9ca355e9711199b5708**（与预期 b7aad4e 一致，以实测为准）。同仓库另有 `phase4b-fix3-a`（同 hash）与 `phase4b-fix3-b`（784e322，未合并，按任务只合 fix3）。
- 合并提交 **904991c**（`Merge tag 'phase4b-fix3' into win`），无冲突。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并与 Windows 专属文件核对

| 项 | 结论 |
|---|---|
| 合并 | 无冲突（0 个 UU/AA/DD） |
| `crates/pty/src/windows.rs` | hash 未变（96416daa…） |
| `crates/pty/tests/pty_windows.rs` | 合并时未变（59f947fd…；本任务 t2 因 clippy 另行修改，见 §2） |
| `docs/WIN-NOTES.md` | 合并时未变（7ad336e1…） |
| `.cargo/config.toml` | hash 未变（311a5385…） |
| `scripts/windows/`、`vt-verify-win/` | 合并 diff 中 0 个文件（`git diff --name-only b8cb0b8 HEAD` 该前缀为空） |

## 2. t2 构建与依赖

| 项 | 结果 |
|---|---|
| debug 构建 | rc=0，25.8s |
| release 构建 | rc=0，**零告警**（修复前有 2 条 `unused variable`，见下） |
| clippy dev `--all-targets -D warnings` | rc=0（修复前 2 处报错：`pty_windows.rs` 的 `single_match`、`app` 的未用参数） |
| clippy release `--all-targets -D warnings` | rc=0 |
| `cargo test --workspace` | rc=0，29 个测试目标全绿 |

修复明细（两处，各自最小）：

1. `shared:` 提交 **a579c36** —— `crates/app/src/lib.rs` 的 `resize_surface(width, height)` 在非 Linux 目标上参数无人使用（GPU 分支仅 Linux），加一行 `#[cfg(not(target_os = "linux"))] let _ = (width, height);`。Linux 路径未改动。
2. `crates/pty/tests/pty_windows.rs` 的 `wait_for`：`match` 单分支改 `if let`（clippy::single_match）。

依赖树证据：

```
cargo tree --target x86_64-pc-windows-gnu -e normal --workspace --prefix none
→ 134 行依赖；去重后 80 个 name@version
→ grep -cE '(^| )(wgpu|pollster)( |$)' = 0        # 无 wgpu、无 pollster
```

- Linux 报告值 205（Linux）/97（Windows）为对照；本机同口径（`-e normal --workspace --prefix none` 去重）实测 **80**，计数口径不同（是否含 dev/build 依赖、是否按版本去重），以实测为准。
- cfg 排除的测试文件（Windows 上编译但 0 个用例）：

| 文件 | 门控 |
|---|---|
| crates/render/tests/background_load.rs | `#![cfg(unix)]` |
| crates/render/tests/baseline_alignment.rs | `#![cfg(unix)]` |
| crates/render/tests/cell_metrics.rs | `#![cfg(unix)]` |
| crates/render/tests/fallback_chain.rs | `#![cfg(unix)]` |
| crates/render/tests/glyph_clip.rs | `#![cfg(unix)]` |
| crates/render/tests/preedit_ghost.rs | `#![cfg(unix)]` |
| crates/render/tests/wide_glyph.rs | `#![cfg(unix)]` |
| crates/render/tests/gpu_parity.rs | `#![cfg(target_os = "linux")]` |

## 3. t3 GPU 配置回落

| 配置 | 窗口 | 进程 | stderr 关键行 |
|---|---|---|---|
| `[renderer] backend = "gpu"` | 正常（找到真窗） | 存活 | **`[renderer] backend=gpu 在当前平台不可用，回退 cpu`** |
| `[renderer] backend = "auto"` | 正常 | 存活 | 无任何 renderer/gpu 行（直接 CPU，未尝试 GPU） |

- 已验证事实：两种配置都**没有** `[perf] gpu_init_ms` 行。
- 已验证事实：没有 `[perf] backend=cpu` 行 —— 代码只在选定后端**不是** cpu 时打印（`crates/app/src/lib.rs:908`），CPU 的判据是"无 backend= 行 + 无 gpu_init_ms"。任务描述里的 `backend=cpu` 行在当前实现中不存在，如实记录。

## 4. t4 CPU 渲染修复在 Windows 上的验证

### 4a 字形基线与顶部裁切

`baseline_alignment.rs` 与 `glyph_clip.rs` 在仓库内是 `#![cfg(unix)]`，Windows 上被排除（原因：这两个文件随 4B-fix1 引入时按 unix 门控，测试体本身是纯 CPU + fontdue，不含 Linux 专属 API）。按任务要求用 `TEST_FONT_FILE` 跑**同一份测试代码**：复制到 `vt-verify-win/probe_render_win/tests/`（仅去掉 `#![cfg(unix)]` 行，不改内容；探针目录不入库，target 已 gitignore）。

字体文件（只用环境变量，不入库）：`C:\Windows\Fonts\consola.ttf`、`C:\Windows\Fonts\msyh.ttc`。

| 字体 | ascent | descent | line_gap | cell | baseline | 越界字符 | 被裁墨迹行 |
|---|---|---|---|---|---|---|---|
| Consolas（主字体） | 9.65 | −3.35 | 2.22 | 8×16 | 11 | 0 | 0（顶部 0） |
| Microsoft YaHei（回退） | 15.08 | −3.74 | 0.00 | 8×16 | 12 | 0 | 0（顶部 0） |
| msyh.ttc 直接作主字体 | 13.76 | −3.40 | 0.00 | 8×18 | 14 | 0 | 0（顶部 0） |

- `baseline_alignment_font_matrix` 与 `glyph_clip_font_matrix` 均 **ok**（release 构建，1.6s / 7.4s）。
- 具体字符：Consolas 下 `min_top=0 (char '|')`、`max_bottom=14`；汉字行 `min_top=0 (char '然')`、`max_bottom=14` —— 顶部无负值即未越界。
- 说明：矩阵里 DejaVu/Liberation/Noto/FreeMono 在本机不存在，测试回落到 Courier New 等替身并仍打印度量（与本任务是验证"公式对"不冲突）。

### 4b 网格外背景填充

用 12px padding 的配置（`dimensions 80×24`，`padding {x=12,y=12}`），客户区 664×408：

- 客户区上边 padding 带中点像素 = **(252,252,252)** = 配置背景色 `#fcfcfc` ✔
- 客户区左上角 (2,2) = (235,235,235)：为窗口边框/阴影像素，非绘制区内，不作为判据。
- 结论：网格外区域按背景色填充，与 Linux case29 语义一致。

### 4c 滚动条 12px

300 行输出后，在右缘 12px 条带内（x = 客户区右 − 6）从轨道中部拖到顶部：

- 视口最大行号 300 → **15**，位置确实变化 ✔（渲染与命中一致）
- 与 2B 的 w2 用例（4px 条带）互为补充：本任务按 3D 规格用 12px 条带重测通过。

### 4d 最小化再恢复：**失败（根因已定位，在共用代码）**

| 观测 | 数据 |
|---|---|
| 恢复后画面与恢复前逐像素比较 | diff = 5413 px（基线噪声：同一窗口连续两次截图 diff = 0） |
| 暗像素数量（文本量） | 恢复前 4465 → 恢复后 2104（约一半内容消失） |
| 强制整屏重绘（`APP_FORCE_FULL_REDRAW=1`） | 同样 diff = 5413（与增量路径完全一致）→ 不是脏区/合帧问题 |
| 按键触发新帧后 | 内容未回来（dark 2330） |
| **最小化期间抓 dump 头** | **`# cols=1 rows=1`**（正常 120×30） |

- 根因（已验证事实）：最小化时窗口收到一次 1×1 的 `WindowEvent::Resized`，`crates/app/src/lib.rs:1233` 的处理器把行列钳到 `min(1)` 并 `vt.resize(1,1)`，VT 内容被裁掉；恢复后行列回到 120×30 但文本已丢失（dump 只剩一个提示符行）。
- 判定：根因在**共用代码**（resize 处理器未过滤最小化/零尺寸），按任务约定不自行修改，写入请求栏（§8 请求 1）。

## 5. t5 release 吞吐（真实 Win32 窗口，release）

判据：批处理文件 `type <file>` 后写完成标记文件，测量从回车到标记出现的时间；同时抓 `APP_PERF_FRAMES`。

### 5.1 2,000,000 行（130 MB）

| 条件 | 尺寸 | 次数 | 用时（s） | render 中位 | present 中位 | interval | coalesced |
|---|---|---|---|---|---|---|---|
| 系统 conhost | 120×30 | 3 | **323.6 / 333.7 / 330.1**（中位 330） | 4 ms | 1 ms | 11–13 ms | 328–373 |
| 系统 conhost | 200×60 | 1 | **325.0** | 11 ms | 5 ms | 39 ms | 351 |
| WezTerm conpty（sideload） | 120×30 | 1 | **229.0** | 3 ms | 1 ms | 11 ms | 329 |

- 已验证事实：release 下 2M 行**能跑完**（约 400 KB/s 级），渲染侧 render/present 占比很小（中位 4/1 ms，约 83–90 fps），瓶颈不在渲染。
- 已验证事实：200×60 用时与 120×30 基本相同（325 s），但 render 中位升到 11 ms、present 5 ms —— 说明瓶颈同样不在渲染量。
- 已验证事实：WezTerm conpty 输出路径比系统 conhost 快约 1.44×（229 s vs 330 s）。
- 偏差：200×60 与 WezTerm 各只跑了 1 次（单次约 5 分钟），未达"各 3 次"；已如实记录。

### 5.2 粘贴不卡死（release）

| 条件 | 1 MB | 10 MB |
|---|---|---|
| 系统 conhost | **1048576 字节，5.7 s** | **10485760 字节，60.7 s** |
| WezTerm conpty | 450 554 字节后停住（300 s 上限）；复测 120 s 停在 456 442 | 459 002 字节后停住（300 s） |

- 已验证事实（系统 conhost）：release 下 1 MB/10 MB 粘贴精确完成、无死锁，app 存活。
- 已验证事实（WezTerm conpty）：三次尝试都停在约 45 万字节（450 554 / 456 442 / 459 002），进程存活但不再前进。
- 待验证：该停滞是 ConPTY 输入侧流控，还是本探针（`ReadFile` 原始读）在新 ConPTY 下的读取行为差异；本轮未区分（两者都会表现为"reader 不再拿到字节"）。
- 已写入请求栏（§8 请求 2）：与 3C 的"输出推进极慢"不同，本轮 release 已证输出侧正常，剩下的是**输入侧**停滞现象。

### 5.3 3C 结论更正

3C 记录的"1 MB 级输出推进极慢而渲染空闲"在 **release 下不成立**（20 000 行 5.47 s、2 000 000 行 330 s 均正常完成）。3C 当时的测量用的是 debug 构建 + 每帧写 dump 快照，属测量方法污染。此项由"缺陷"降级为"测量方法问题"，已在 WIN-NOTES 更正。

## 6. t6 启动打点（release，5 次）

| 配置 | config | window | surface | pty_spawn | fontdb | 字体组 | first_frame | bg_fonts |
|---|---|---|---|---|---|---|---|---|
| 默认 | 27 (24–30) | 62 (59–66) | 1.2 | 10 (8–13) | 44 (41–56) | 136 (123–145) | **701**（678–1558，1 次 1558 为离群） | 807 |
| examples/windows.toml | 1 (1–2) | 64 (63–66) | 1.0 | 78 (47–94) | 48 (42–51) | 133 (129–136) | **823**（710–6547） | 1438 |

- 可解释部分（默认中位）≈ 27+62+1+10+44+136 = **280 ms**；first_frame 701 ms → **剩余约 420 ms 无打点覆盖**。
- 外部定位（不改共用代码）：用 `Start-Process` 记创建时刻 + `EnumWindows` 轮询真窗出现，5 次结果 `windowVisible = 745/958/765/791 ms`（另 1 次 2740 ms，因 `Start-Process` 本身耗时 2074 ms，属系统侧抖动），与 `first_frame` 相差仅 7–116 ms。
- 结论（工程判断）：剩余 ~420 ms 发生在**进程启动到窗口可见之间**（winit 事件循环初始化 + 首帧合成），不是窗口可见之后的重绘；要精确拆分需在共用代码补两个打点（renderer/字形缓存初始化完成、首帧提交前）。写入请求栏（§8 请求 3）。

## 7. 未解决问题

1. t4d：最小化 → 1×1 resize → VT 内容被裁（§4d）。根因在共用代码，未修，按规则交 Linux 侧。
2. WezTerm conpty 下 >450 KB 的粘贴停滞（§5.2），是否探针读取方式所致未区分。
3. 2M 行吞吐的 200×60 与 WezTerm 条件各只有 1 次样本（单次约 4–5 分钟）。
4. 启动剩余 ~420 ms 未拆分（§6），需要新打点。

## 8. 对共享代码的请求

1. **（P1，数据丢失）** `crates/app/src/lib.rs:1233` 的 `WindowEvent::Resized` 未过滤最小化/零尺寸：最小化时收到 1×1，`min(1)` 钳位后 `vt.resize(1,1)` 把终端内容裁掉，恢复后无法还原。Windows 实测：最小化期间 dump 头为 `# cols=1 rows=1`，恢复后文本丢失（暗像素 4465→2104）。建议在 cols/rows 为 0（或 `physical_size` 任一维为 0）时跳过 vt/pty resize，或记住最小化前的尺寸。
2. **（输入侧，现象）** WezTerm conpty 1.23/20240203 下，向原始读端粘贴 >450 KB 时停在约 450 000 字节不再前进（3 次复现），系统 conhost 正常完成 1 MB/10 MB。是否为 ConPTY 输入流控或本探针读取方式差异未定位；建议在共享的读线程→channel→vt.feed 路径加"每秒喂入字节数"观测（属观测增强）。
3. **（观测增强）** 启动分段仍缺两点打点：renderer/字形缓存初始化完成、首帧提交前；Windows 实测剩余约 420 ms 落在这段（窗口可见与首帧相差仅 7–116 ms）。
4. （已在本轮修复，仅备案）`resize_surface` 在非 Linux 目标的未用参数告警 → a579c36。

## 9. 交付物与证据

- 提交：**a579c36**（shared: 非 Linux 目标忽略 resize_surface 未用参数）+ 本轮主提交（文档/脚本/测试 clippy 修复）。
- 脚本：`vt-verify-win/{run_backend,run_t4bcd,run_t4d_diag,run_t4d_diag2,run_t4d_dumpmin,run_t4d_final,run_throughput2,run_paste2,run_t6_ext,run_perf3c}.ps1`；`vt-verify-win/probe_render_win/`（跑被 unix 门控的 baseline_alignment / glyph_clip）。
- 配置：`scripts/windows/e2e/cfg/{backend-gpu,backend-auto,pad12,tp-120x30,tp-200x60}.toml`。
- 日志/截图：`vt-verify-win/logs/{3d-clippy-dev,3d-clippy-rel,3d-test,3d-tree*,3d-baseline-*,3d-glyphclip-*,3d-perf}.log`、`vt-verify-win/logs/phase2b-win/{t4b-padding,t4d-*,t4df-*,backend-*}.{png,err,dump}`。
- 未入库：`vt-verify-win/perfdata/`（含 130 MB 测试文件）、`vt-verify-win/probe_render_win/target/`、任何 conpty.dll/OpenConsole.exe。

## 9.5 推送与打 tag

- `git push origin win`：`b8cb0b8..dfa882a  win -> win`；`git rev-parse win` = `dfa882ac5d93c2d0d48b2c9ed31c33020c9b11d3`，`git ls-remote origin refs/heads/win` 同值。
- 提交序列：a579c36（shared: 非 Linux 目标忽略未用参数）→ dfa882a（本轮主体：合并、脚本、测试 clippy 修复、文档）。
- tag：`phase3d-win-done`（annotated，指向 dfa882a，tag 对象 `1f8c6eebd3514c89abfc0dc2e2881a1e8e5d8ec6`），已 push 并在 ls-remote 可见。

## 10. 手动清单（≤3）

1. 用真实输入法在 Consolas + Microsoft YaHei 回退下输入中文，确认顶部与下行笔画无裁切（本机只做了字形级与像素级统计）。
2. 用 Windows Terminal 复核 WezTerm conpty 的粘贴停滞是否同样出现（本机只有这两种 ConPTY）。
3. 若关注启动耗时：把 `renderer.backend` 显式设为 `cpu`，体感与本机默认配置的差异（本机数据见 §6）。
