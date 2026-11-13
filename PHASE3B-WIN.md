# PHASE3B-WIN：阶段3C Windows 复验交付

- 日期：2026-11-13（按系统时间）
- 范围：合并 phase3c-done；修 Windows pty 测试；评估 `clear_console_ctrl_ignore` 副作用；新版 ConPTY 完整回归；补启动分段与吞吐数据。未做 GPU。
- `git rev-parse phase3c-done` = **2d7e60592f728bdbe9a9feddc7ffe3ee9cb295a9**；合并为**快进**（4bd466c → 2d7e605），无冲突。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（windows-gnu）；Zig 0.17.1；portable-pty 0.8.1；pwsh 7.6.6 + PSReadLine 2.4.5。
- 结论分级：已验证事实 = 本机实测；工程判断 = 由事实推断；待验证假设 = 未证实。

## 1. 构建与测试

| 项 | 结论 | 证据 |
|---|---|---|
| debug 构建 | rc=0，24.3s | vt-verify-win/logs/3c-build-*（当轮输出） |
| release 构建 | rc=0，24.5s，**零告警**（上一轮 `t_present` 告警已由 3C 消除） | 同上 |
| clippy dev（`--all-targets -D warnings`） | rc=0，0 warning | vt-verify-win/logs/3c-clippy-dev.log |
| clippy release（`--release --all-targets -D warnings`） | rc=0，0 warning | vt-verify-win/logs/3c-clippy-rel.log |
| cargo test --workspace | **rc=0，29 个测试目标全绿，3 连跑稳定** | vt-verify-win/logs/3c-test-after2.log、/tmp/ws2.log、/tmp/ws3.log |

修复前记录：`cargo test --workspace` rc=130，`pty_windows::ctrl_c_probe_powershell_ping` 失败并使 harness 以 `0xC000013A` 退出（vt-verify-win/logs/3c-test-before.log）。

## 2. 任务2：Windows pty 测试修复（crates/pty/tests/pty_windows.rs）

### 2.1 时序修复（已验证事实）

`ctrl_c_probe_powershell_ping` 原本用 `drain_quiet(静默 800 ms, 上限 6 s)` 取首段输出。本机实测：powershell 冷启动 ≈ 1167 ms，ping 的 TTL 间隔 ≈ 1 s，两者都大于 800 ms，故首段只拿到 banner 就返回。

改为 `wait_for(rx, b"TTL=", 15 s)`（等到目标字节或总超时），并**加强**中断断言：Ctrl-C 之后等待提示符 `PS ` 出现，断言提示符之后不再出现任何 `TTL=`。断言只加不减。

同类隐患一并修（都是固定静窗 + 慢启动）：`pty_cmd_echo_cn_resize_close`、`pty_powershell_echo_cn`、`pty_default_shell_is_cmd`、`ctrl_c_probe_cmd_ping_twice` 的预取段改为 `wait_for`。修完全套 `cargo test --workspace` 3 连绿（29 目标）。

### 2.2 0xC000013A 的归属（已验证事实，推翻上一轮结论）

- 对照组 A（有 `reignore_ctrl_c_in_test()`，即 spawn 后把本进程的"忽略 Ctrl-C"标志重新置上）：**5/5 通过**。
- 对照组 B（`TTY_PTY_NO_REIGNORE=1`，保留原行为）：**5/5 通过**。
- 结论：`ctrl_c_probe_powershell_ping` **单独运行与 `clear_console_ctrl_ignore` 无关**，不是它导致退出。
- 真因（逐个单跑 11 个未完成测试定位）：**`t4_e8_ffi_ctrl_c_to_ping`** 调用 `AttachConsole(pid)`（ConPTY 拒绝附加，返回 0）后执行 `GenerateConsoleCtrlEvent(0, 0)`，进程组 0 = **调用者自身所在控制台组**，于是测试进程收到 Ctrl-C 并以 `STATUS_CONTROL_C_EXIT` 退出；并行跑整套时表现为"跑完 10 个就死"。
- 修复：在该测试里、调用 `GenerateConsoleCtrlEvent` 之前先 `reignore_ctrl_c_in_test()`（仅测试进程，不改 `windows.rs` 的 spawn 语义）。修后单跑通过，整套 21 个测试 4 连绿（/tmp/FP2..FP4、FQ1..FQ4）。
- 结论只写数据：`windows.rs` 未改动。

## 3. 任务3：`clear_console_ctrl_ignore` 对 app 的副作用（已验证事实）

方法：用 `cmd /c start "唯一标题" <host>` 新开一个控制台窗口，在其中运行 release `app.exe`；按标题找到**该**宿主窗口并聚焦，用 keybd_event 向宿主窗口发送真实 Ctrl+C（从不向 app 窗口发）。以"启动前后 app PID 差集"确定本用例启动的实例。

| 宿主 | 次数 | app 是否退出 |
|---|---|---|
| cmd.exe | 3 | 3/3 **存活**（PID 不变） |
| powershell.exe | 3 | 3/3 **存活**（PID 不变） |

- 结论：`clear_console_ctrl_ignore()` 会让 app 进程自己不再忽略 Ctrl-C，但**实测没有观察到**向宿主控制台发 Ctrl+C 导致 app 退出。无需改行为，不进请求栏。
- 早期一次"app 被杀死"的观察经复核是残留 PID 误判（残留实例被算进前后集合），已作废。

## 4. 任务4：新版 ConPTY 完整回归（只评估，未入库）

### 4.1 对照件（只读复制到测试二进制旁，目录 gitignore）

| 来源 | 文件 | 大小 | sha256 |
|---|---|---|---|
| WezTerm 20240203（`E:\Compiler\WezTerm`，wezterm-gui 20240203-110809-5046fc22） | conpty.dll | 99 328 | `2f09eaa55c60e11241ca21fff19336529470d9b76a77bcb45de78cfabdb50308` |
| 同上 | OpenConsole.exe | 1 395 712 | `6b0e73145462116b2ed3d422ac71e25c8554b1a52295d8cf55cf6025775276ee` |
| node-pty prebuild（`@lydell/node-pty-win32-x64`，conpty.dll/OpenConsole.exe 版本 1.23.2510.08001） | conpty.dll | 109 600 | `7c7430632052ff703540b68371ec43821820aa1335d8e11dfbcd9ff00e9daaed` |
| 同上 | OpenConsole.exe | 1 148 448 | `d1fe7faa62f9e955e2ac2371f95d7e5513df4d496255097158f979c94782c5fc` |

- 许可：文件来源未核实（WezTerm 目录内无 LICENSE 文件；node-pty 包内未核对许可文本）。

### 4.2 三条件完整回归（debug 二进制，条件差异仅体现在同目录的 conpty 对）

| 条件 | run_all | 2B clip | 2B mouse | 2C | 失败用例 |
|---|---|---|---|---|---|
| 系统 conhost（19044） | **14/14** | 10/11 | **6/6** | **8/8** | m2（双击退化为单格，注入时序） |
| WezTerm conpty 20240203 | 13/14 | 10/11 | 6/6 | 8/8 | d-backspace-arrows（丢首字符）、m2 |
| node-pty conpty 1.23.2510 | 13/14 | **11/11** | 5/6 | 8/8 | d-backspace-arrows（丢首字符）、w3（bottom=-1，注入未生效） |

- 失败用例的 dump/截图保留在 `scripts/windows/e2e/out/`（case-d.dump 显示 `C:\Users\Admin>cd`，即首个 `a` 未到达）。
- d 用例的现象：两个非系统条件各出现 1 次、系统条件 2 次未出现；样本小，**不能判定为新 ConPTY 特有**，列为待验证（注入侧"首个按键被吞"在 2B/2C 也出现过）。
- m2/w3 属既有的注入时序抖动（2B 起就有），与 conpty 版本无关（系统条件下 m2 也出现）。

### 4.3 专项

| 项 | 系统 conhost | WezTerm conpty |
|---|---|---|
| Ctrl-C 中断 ping -t（run_all i/j） | 通过 | 通过 |
| Ctrl-C 中断 Start-Sleep（run_all k/m） | 通过 | 通过 |
| sort 读 stdin 被 Ctrl-C 打断（l） | 通过 | 通过 |
| resize 后网格与 `mode con` 一致（g） | 通过 | 通过 |
| 汉字宽格（c） | 通过（wide=2 spacer=2） | 通过（wide=2 spacer=2） |
| 软折行复制（2C c1） | 通过（首段 118，说明折行被物化） | 通过（首段 176，更接近整行拼接） |
| shell=pwsh.exe 连续启动 | 见下 | 见下 |
| OSC 0 标题生效 | 见下 | 见下 |

- pwsh 连续启动 20 次存活率：**20/20**（系统 conhost；`pwsh-win.toml`，每次等窗口出现后判定进程存活）。**未在新版 conpty 下重复**（时间所限，列为未覆盖）。
- OSC 0 标题：上一轮 5 次中 2 次生效；本轮 c4a 在三种条件下均按"观测项"记录，系统中出现"标题由 app 初始值变为 conhost 中介值"（changed=True），仍未观察到稳定的 T123。

### 4.4 差异表（结论）

| 维度 | 系统 conhost | 新版 ConPTY（两者一致） | 是否有差异 |
|---|---|---|---|
| 折行是否物化为 CRLF | 是（A×80 CRLF A×80 CRLF A×40） | 否（单段 200 字节） | **有**（上一轮已证，本轮复验仍成立） |
| 括号粘贴标记 | 吞掉（子进程只收到 AB） | 吞掉 | 无 |
| SGR 鼠标序列 | 吞掉（0 字节） | 吞掉 | 无 |
| e2e 通过数 | 38/39 | 37/39（WezTerm）、37/39（node-pty） | 差异集中在 d 用例的首字符丢失（样本小，待验证） |

- 结论：新版 ConPTY 对**折行**有实质改善，对其余两项无改善；完整回归**未见明确回归**，唯一疑似差异是 d 用例的首键丢失，需更多样本确认。

## 5. 任务5：启动分段与吞吐

### 5.1 启动分段（release，各 5 次，ms，括号内为 5 次原始值）

| 配置 | config | window | surface | pty_spawn | fontdb | 字体组 | first_frame | bg_fonts | first_pty_byte |
|---|---|---|---|---|---|---|---|---|---|
| 默认 | 25.4 (21.7–72.7) | 107 (75–122) | 12 (7–36) | 9.1 (8.1–26) | 53.3 (50–100) | 158 (127–162) | **823** (749–1586) | 951 | 1061 (870–1669) |
| examples/windows.toml | 1.0 (0.8–1.1) | 94 (72–111) | 21 (1–41) | 74 (33–111) | 48 (37–62) | 147 (123–168) | **844** (743–1220) | 1476 | 1238 (770–1267) |

**819 ms 的构成（已验证事实 + 剩余量）**：config 25 + window 107 + surface 12 + pty_spawn 9 + fontdb 53 + 字体组 158 ≈ **364 ms**，剩余 **约 460 ms 未被现有 `[perf]` 打点覆盖**。

- 已验证事实：`first_pty_byte_ms`（≈1061）普遍**晚于** `first_frame_ms`（≈823），说明首帧不等 shell 输出，剩余量不在 PTY 上。
- 已验证事实：app 自身首帧后渲染极快（release `render_ms` 中位 1–3 ms、`present_ms` 中位 1–2 ms），剩余量也不在稳态渲染。
- 待验证假设：剩余量在 winit 事件循环启动 + 首帧合成/字形缓存预热之间；需要新增打点（例如 renderer 初始化完成、首帧提交前）才能进一步分解。未实施。

### 5.2 吞吐（未取得可用数值，记录现象与证据）

- 文件：`vt-verify-win/perfdata/big20k.txt`（1.28 MB / 20 000 行 + `ZZENDMARK`）、`big200k.txt`（12.8 MB / 200 000 行），目录已 gitignore。
- 命令：app 内 cmd 执行 `type "<绝对路径>"`（避免 `&`，合成键入的 Shift 组合会污染命令行）。
- 结果（同一 debug 二进制，以便用 dump 判定结束标记）：
  - 20 000 行：180 s 内未出现 `ZZENDMARK`；dump 顶部停在 `L000050` 附近。
  - 100 行：60 s 内未出现 `ZZENDMARK`。
- 同时段的帧数据（`APP_PERF_FRAMES=1`）：release `render_ms` 中位 1–3 ms、`present_ms` 中位 1–2 ms、`interval_ms` 8–12、约 83–125 fps、`coalesced` ≈ 130；debug 为 16–18 ms / 29 fps。
- 记载：**未测得吞吐数值**；现象（输出推进极慢、渲染侧却很空闲）与"PTY 输出喂入 vt 被帧节奏节流"一致，属待验证假设；对照实验（系统 conhost vs 新版 ConPTY）因此未能完成，列入未解决问题。
- 时间成本：该测量尝试耗时约 25 分钟（200k/20k/100 行共 6 次运行），未继续扩大。

## 6. 未解决问题

1. 吞吐测量未取得有效数值（§5.2）；200k 行在 300 s 内未完成，20k 行 180 s 内推进约 50 行。原因未定位（待验证：喂入节流 / ConPTY 传输），因此未能给出系统 conhost 与新版的吞吐对照。
2. `d-backspace-arrows` 在两种新版 ConPTY 条件下各失败 1 次（首字符丢失），系统条件未复现；样本小，未定位。
3. 新版 ConPTY 下 pwsh 连续启动 20 次未测（仅系统 conhost 测了 20/20）。
4. 启动分段仍有约 460 ms 无打点覆盖（§5.1）。
5. OSC 0 标题在 Windows 上仍不稳定（conhost 中介），三种条件都按观测项记录。

## 7. 对共享代码的请求

1. （3A 已提，本轮关闭）`crates/app/src/lib.rs` 的 `t_present` release 告警：3C 已消除，无需再跟踪。
2. （新）启动分段建议再补两个打点：renderer/字形缓存初始化完成、首帧提交前，用于解释 §5.1 中约 460 ms 的缺口。属观测增强，不改变行为。
3. （新，待验证）吞吐现象：大段输出（1 MB 级）在 app 内推进极慢而渲染侧空闲（§5.2）；建议在 PTY 读线程→channel→vt.feed 的路径上统计每秒喂入字节数，确认是否存在节流。当前仅为现象记录，未定位。

## 8. 交付物与证据

- 代码：`crates/pty/tests/pty_windows.rs`（等待策略改写、`reignore_ctrl_c_in_test`、E8 修）；`scripts/windows/e2e/run_conpty_condition.ps1`（三条件驱动）；`scripts/windows/e2e/run_2b_clip.ps1`（m2/m3 预热重试）。
- 脚本：`vt-verify-win/{run_ctrl_host3,run_perf3c,run_throughput,run_case_d}.ps1`。
- 日志：`vt-verify-win/logs/{3c-test-before,3c-test-after2,3c-clippy-dev,3c-clippy-rel,3c-cond-system2,3c-cond-wez,3c-cond-node,3c-perf3,3c-tp-*,3c-ctrlhost-7}.log` 与 `vt-verify-win/logs/conpty-runs/`。
- 未入库：`vt-verify-win/conpty-test/`、`vt-verify-win/perfdata/`（均已在 .gitignore）。

## 9. 推送核对

- 提交：**7c6d2ab**（`3C-win: merge phase3c-done, fix Windows pty tests, ConPTY full regression`），130 文件，+4918/-550。
- `git push origin win`：`4bd466c..7c6d2ab  win -> win`。
- `git rev-parse win` = 7c6d2ab6ade7ca4e6472b8dade3ae50313ddfc69；`git ls-remote origin refs/heads/win` = 7c6d2ab6ade7ca4e6472b8dade3ae50313ddfc69（一致）。

## 10. 手动清单（≤3）

1. 用 WezTerm（E:\Compiler\WezTerm）自带的 pwsh 7 跑一次大文件 `type`（例如 20k 行），确认 §5.2 的"推进极慢"是否只在 ConPTY + 本 app 组合下出现。
2. 在装有 Windows Terminal 的机器上复核 §4.3 的 Ctrl-C / resize / 汉字宽格三项。
3. 若计划集成新版 conpty.dll：先核对 WezTerm/node-pty 两个来源的许可与再分发条款（本机未找到 LICENSE）。
