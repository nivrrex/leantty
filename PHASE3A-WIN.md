# PHASE3A-WIN：阶段3A Windows 复验交付

- 日期：2026-11-13（按系统时间）
- 范围：合并 Linux 阶段3A（tag phase3a-done）后在 Windows 复验增量渲染、取启动分段数据、做 ConPTY 版本对照。仅 CPU，未做 GPU，未合入 3B 中间态。
- 基线：合并前 win = 41dc491；`git rev-parse phase3a-done` = **f32b9860d8444fca8ec82b79ff4d4006987d2614**；合并提交 **d4006a4**（非快进，win 侧 2D 提交与 3A 分叉）。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（windows-gnu）；Zig 0.17.1；portable-pty 0.8.1；pwsh 7.6.6 + PSReadLine 2.4.5。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实推断；待验证假设 = 未证实。

## 1. 合并、构建与测试

| 项 | 结论 | 证据 |
|---|---|---|
| merge | 无冲突（0 个 UU/AA/DD），提交 d4006a4 | `git merge phase3a-done` |
| debug 构建 | rc=0，56.6s | 构建日志（本次调用输出） |
| release 构建 | rc=0，24.3s；**产生 1 条 warning**：`unused variable: t_present`（crates/app/src/lib.rs:912，仅在 `#[cfg(debug_assertions)]` 下使用） | 同上 |
| clippy --all-targets -D warnings | rc=0，13.2s（dev profile 下该变量被使用，故不触发） | vt-verify-win/logs/3a-clippy.log |
| cargo test --workspace | **config 的 `path_priority_cli_env_default` 已转绿**（10 passed）；仅 `pty_windows::ctrl_c_probe_powershell_ping` 失败并让 harness 以 0xC000013A 退出 | vt-verify-win/logs/3a-test.log、3a-ping-test.log、3a-pty-serial.log |
| 增量渲染单测 | `incremental_matches_full_pixel_exact`、`incremental_random_sequence_2000_steps` 均通过，2 项合计 **51.18s** | vt-verify-win/logs/3a-incremental.log |
| 换种子 | `APP_RANDOM_SEED=987654321` 重跑 2000 步：通过，**53.19s**，输出"增量与全量逐像素一致" | vt-verify-win/logs/3a-incremental-seed.log |

`ctrl_c_probe_powershell_ping` 的实际失败信息：

```
powershell ping -t 应有 TTL 输出，实际 "\u{1b}[2J\u{1b}[m\u{1b}[H\u{1b}]0;…powershell.exe…"
```

- 已验证事实：本机 powershell.exe 冷启动约 **1167 ms**（3 次平均，`Start-Process` + `WaitForExit`），而该测试的 `drain_quiet(800ms 静默, 6s 总)` + `ping` 的 TTL 间隔约 1 s 都超过 800 ms 静默阈值，预取阶段只拿到 banner。串行执行（`--test-threads=1`）同样失败，排除并发干扰。
- 已验证事实：该测试所在的 `crates/pty` 在 3A 合并中**无任何改动**（`git diff --stat 41dc491..HEAD -- crates/pty` 为空）。
- 判定：**共享测试的时序假设在本机（慢启动）不成立**，非 3A 引入，未擅改（请求 1）。

## 2. 回归 e2e（全部脚本重跑）

| 套件 | 结果 | 证据 |
|---|---|---|
| run_all.ps1（2A 全矩阵，含 i/j/k/l/m） | 14 PASS / 0 FAIL | vt-verify-win/logs/3a-runall-1.log |
| run_2b_clip.ps1 | 11 PASS / 0 FAIL（含静止 1MB = 1048576 精确） | vt-verify-win/logs/3a-clip-1.log |
| run_2b_mouse.ps1 | 6 PASS / 0 FAIL | vt-verify-win/logs/3a-mouse-1.log |
| run_2c_win.ps1 | 8 PASS / 0 FAIL（c4a 改为观测项，见 §3.4） | vt-verify-win/logs/3a-2c-3.log |

## 3. 3A 复验

### 3.1 增量渲染对拍（单测）

见 §1：同一实现两份数据（默认种子 51.18 s、种子 987654321 53.19 s），两步法一致性断言在 Windows 下通过。

### 3.2 截图逐像素对拍（run_3a_win.ps1 a1）

同一输入序列（40 行 echo + 一次回车），分别以默认增量与 `APP_FORCE_FULL_REDRAW=1` 运行，取 app 真窗**客户区**截图逐像素比较：

- 结果：**diffPixels = 0**，两图字节数相同（15089 / 15089）。
- 证据：vt-verify-win/logs/phase2b-win/a1-incremental.png、a1-forced-full.png；日志 3a-render-2.log。
- 说明：截图前先让画面静止 1.5 s，避免光标闪烁造成假差异。

### 3.3 present 路径（a2，debug + `APP_PRESENT_STATS=1`）

| 路径 | 采样组 | median | p95 |
|---|---|---|---|
| damage（默认） | 2 组 ×20 帧 | **149 µs / 197 µs** | 32.3 ms / 30.9 ms |
| 全缓冲（`APP_PRESENT_FULL=1`） | 3 组 ×20 帧 | **31.7 / 31.5 / 32.4 ms** | 39.8 / 49.2 / 44.1 ms |

- 已验证事实：本机（GDI/softbuffer）下 damage 提交的**中位**比整缓冲快约 200 倍；p95 两者同量级（30–49 ms），说明偶发的大块提交占主导。
- 只记录，不优化。

### 3.4 2000 行输出后的响应性（a3）

- 2000 行 echo 完成（dump 见到 B2000）后立即测 20 次键入回显：**中位 453 ms、p95 583 ms、最大 583 ms**（debug 构建；每次样本含 `Send-Text2` 4 字符 × 40 ms 的注入开销，故实际回显延迟低于该值）。
- 结论：窗口在洪峰后保持响应，无冻结。

### 3.5 2C 复验中的观测项变化（c4a）

- 现象：子进程发 `ESC]0;T123 BEL` 后，窗口标题**有时**变 T123（5 次中 2 次），多数时候停在 conhost 中介后的 `Administrator: C:\Windows\system32\cmd.exe`。
- 已验证事实：vt 层解析正常（probe_title：`ESC]0;T123 BEL` → Some("T123")，`ESC]2;T456 ST` → Some("T456")）；app 侧也确实跟随过 OSC（本次观测到标题由 `C:\Windows\system32\cmd.exe - chcp 65001` 变为 `Administrator: …`，changed=True）。
- 判定：属 conhost 对子进程 OSC 0 的中介/丢弃，**不是 app 缺陷**。c4a 因此改为观测项（记录 initial/final/sawT123），其余 7 例仍严格断言。

## 4. 启动分段（release，各 5 次，单位 ms）

| 配置 | fontdb | 字体组合计 | window | surface | first_frame | bg_fonts |
|---|---|---|---|---|---|---|
| 默认（无 --config） | 68.8 | 153.4 | 115.9 | 11.0 | **819** | 972 |
| examples/windows.toml（Consolas + YaHei 回退，shell=pwsh） | 65.6 | 142.2 | 100.2 | 20.4 | **851** | 1534 |

原始 5 次（默认）：fontdb 66.3/68.8/83.2/93.4/68.0；first_frame 819/1245/798/799/874；bg 972/1370/929/952/1057。

**上一轮 1465 ms 的构成（已验证事实）**：2D 那轮用的是 `clip-win.toml`（主字体 Microsoft YaHei Mono），本轮同配置的**字体组加载合计约 1275 ms**（5 次 1234–1356），加上 fontdb ~60、window ~100，合计约 1435–1520 ms —— 即**主字体 4 组加载占绝对多数**，不是 fontdb、也不是窗口/表面创建。

- 本机字体规模（只读统计）：`C:\Windows\Fonts` **477 个文件、600.7 MB**；用户字体目录 22 个。
- fontdb 扫描仅 ~66 ms，**不占大头**，故未进一步展开。
- 建议（工程判断）：默认主字体用 Consolas、CJK 走 Microsoft YaHei 回退；YaHei Mono 作主字体时首帧多付约 1.1 s。

## 5. ConPTY 版本对照（只评估，未改仓库代码，未分发二进制）

### 5.1 定位（只读）

| 项 | 结果 |
|---|---|
| WezTerm | `E:\Compiler\WezTerm`，`wezterm-gui.exe` 版本 `20240203-110809-5046fc22`；同目录有 `conpty.dll`（99 328 B，2024-02-04）与 `OpenConsole.exe`（1 395 712 B，2024-02-04），两者无版本资源 |
| 另一份新 conpty（对照补充） | `%APPDATA%\npm\node_modules\openclaw\node_modules\@lydell\node-pty-win32-x64\prebuilds\win32-x64\conpty\`，conpty.dll / OpenConsole.exe 版本 **1.23.2510.08001** |
| 系统 | `C:\Windows\System32` 无 conpty.dll / OpenConsole.exe（只有 conhost.exe），即默认走内核 conhost（19044） |

### 5.2 portable-pty 0.8.1 源码行为（只读查阅）

- `src/win/psuedocon.rs:44-61` `load_conpty()`：先 `ConPtyFuncs::open("kernel32.dll")` 确认系统支持；**随后尝试 `ConPtyFuncs::open(Path::new("conpty.dll"))`**，成功即用该 sideload 版本，否则回落 kernel32。
- 解析路径为相对名 "conpty.dll"，最终走 `shared_library 0.1.9` 的 `LoadLibraryW`（`src/dynamic_library.rs:340`），即标准 DLL 搜索顺序：**程序目录优先**、再系统目录与 PATH。
- 结论：把 conpty.dll（连带 OpenConsole.exe）放在 app.exe 同目录即可启用新版 ConPTY，无需改库。

### 5.3 实验（探针与 app 同目录放入 WezTerm 的 conpty.dll + OpenConsole.exe，目录已 gitignore）

| 场景 | 系统 conhost（19044） | WezTerm conpty 20240203 | 差异 |
|---|---|---|---|
| 200 个 A（PTY 80 列）折行 | `A×80 CRLF A×80 CRLF A×40`（换行被物化） | **单段 200 字节，无 CRLF** | **有改善**：软折行保留 |
| 括号粘贴 `ESC[200~AB ESC[201~` | 子进程收到 `AB`（6→2 字节，标记被吞） | 子进程收到 `AB` | 无差异 |
| SGR 鼠标 `ESC[<0;10;5M` | 子进程收到 0 字节 | 子进程收到 0 字节 | 无差异 |
| pwsh 7.6.6（flags 0/1/6 × 尺寸 80×24/120×30/200×50，共 9 组合抽样） | 全部 alive、无 totalWidth | 全部 alive、无 totalWidth | 无差异（本轮两版都不崩） |
| node-pty conpty 1.23.2510（对照） | — | 折行同样不物化（单段 200 字节）；括号粘贴仍只到 `AB`；SGR 仍 0 字节 | 与 WezTerm 版同现象 |

端到端复核（app + sideload，debug 构建，同一选中区域）：

| conpty | 复制的 A 串最大连续长度 |
|---|---|
| 系统 conhost | **118**（在 120 列折行处被换行切断） |
| WezTerm conpty | **198**（软折行被正确拼接，少 2 是选区起点差 2 列） |

### 5.4 评估（结论只写差异，不实施）

- **差异是什么**：新版 ConPTY 不再把折行物化为 CRLF，因此 2C.1 的软折行拼接在 Windows 上**变为可触发**；括号粘贴标记与 SGR 鼠标序列仍被 ConPTY 输入通道吃掉，未改善；pwsh 崩溃本轮两版都不复现，无法归因。
- 集成方案选项（工程判断，均未实施）：
  1. 在发布目录旁随附 `conpty.dll` + `OpenConsole.exe`，靠 portable-pty 已有的 sideload 逻辑自动启用——零代码改动。
  2. 若不想随附，维持系统 conhost，代价是 Windows 上软折行拼接不可达（功能降级但不影响正确性）。
  3. 不推荐改便携库或 fork portable-pty：现有加载路径已满足需求。
- 分发与许可注意（**待核实**）：这两个二进制在上游来自 Windows Terminal 的 OpenConsole/ConPTY，通常以 MIT 发布；本机 WezTerm 目录内**没有 LICENSE 文件**可直接佐证，具体再分发条款与版本对应关系需在集成前确认。另注意 DLL 搜索顺序带来的安全的副作用（程序目录优先，目录需防写）。
- 已写入 `docs/WIN-NOTES.md` 请求 8（评估项，不实施）。

## 6. 未解决问题

1. `pty_windows::ctrl_c_probe_powershell_ping` 在本机稳定失败（时序假设，见 §1），并使测试 harness 以 0xC000013A 退出；共享测试，未改。
2. release 构建的 `unused variable: t_present` 警告（crates/app/src/lib.rs:912）——只在 release 出现，clippy（dev）看不到；共享代码，请求 2。
3. 子进程 OSC 0 标题在 Windows 上经 conhost 中介后不稳定（5 次中 2 次生效），见 §3.5；属平台行为，记录不作回归项。
4. ConPTY 新版可改善折行，但括号粘贴与 SGR 上报在 Windows 仍不可端到端验证。

## 7. 对共享代码的请求（已写入 docs/WIN-NOTES.md）

1. `crates/pty/tests/pty_windows.rs::ctrl_c_probe_powershell_ping`：`drain_quiet` 的 800 ms 静默阈值小于 ping 的 TTL 间隔（~1 s）与本机 powershell 冷启动（~1.17 s），预取阶段拿不到 TTL；建议把静默阈值提到 ≥1.5 s 或改为"等到首个 TTL 或超时"。状态=已验证事实。
2. `crates/app/src/lib.rs:912` 的 `t_present` 仅在 debug 分支使用，release 构建产生 `unused variable` 警告；建议加 `#[cfg(debug_assertions)]` 或 `let _ = t_present;`。状态=已验证事实。
3. 观察项（非阻塞）：ConPTY 新版不物化折行（§5.3），若将来需要 Windows 上验证软折行拼接，sideload conpty.dll 即是最小成本路径；当前不实施。

## 8. 交付物与证据

- 新增脚本：`scripts/windows/e2e/run_3a_win.ps1`（增量/全量截图对拍、present 统计、2000 行响应）、`run_3a_perf.ps1`（启动分段）、`vt-verify-win/{probe_title,c1-sideload,title-diag3,echo-diag}.ps1`、`probe_title/`、`vt-verify-win/osc123loop.ps1`。
- 日志：`vt-verify-win/logs/{3a-test,3a-ping-test,3a-pty-serial,3a-incremental,3a-incremental-seed,3a-clippy,3a-runall-1,3a-clip-1,3a-mouse-1,3a-2c-3,3a-render-2,3a-perf-2}.log`、`wrap_sys.txt`。
- 截图：`vt-verify-win/logs/phase2b-win/{a1-incremental,a1-forced-full,c3-plain,c3-preedit}.png`。
- ConPTY 对照件：`vt-verify-win/conpty-test/`（已 gitignore，含 WezTerm 的 conpty.dll/OpenConsole.exe 与探针副本），未入库。

## 9. 手动清单（≤3）

1. 用 WezTerm（E:\Compiler\WezTerm）里的 pwsh 7 跑一次常见操作（vim/less/neovim），确认折行为与 3A 的软折行语义一致（本机只做了 PTY 字节层对照）。
2. 在装有 Windows Terminal 的机器上复核 §5.3 的两项"无差异"结论（括号粘贴、SGR），确认是否与 conpty 版本无关。
3. 若计划随附 conpty.dll/OpenConsole.exe：先确认上游许可与再分发条款，再做一次杀软/签名检查（本机未做）。
