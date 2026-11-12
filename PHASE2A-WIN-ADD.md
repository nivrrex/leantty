# PHASE2A-WIN-ADD：Ctrl-C 继承假设实测 + pwsh FailFast 层级隔离

- 日期：2026-11-11（按系统时间）
- 基线：631587e 之上的 win 分支（319f438）；未合并 origin/main 的 2B 增量；仅 CPU。
- 分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。
- AGENTS.md 冲突说明：同 PHASE2A-WIN.md（handoff-gate 节自述仅 report_to_user 可用时适用，本机无该工具），无实质冲突。

## 1. 任务1：Ctrl-C 继承假设（用户给定假设：app 继承"忽略 Ctrl-C"标志，致子进程不响应控制台 Ctrl-C 事件）

### 实验 X（代码侧，windows.rs spawn 前调用 SetConsoleCtrlHandler(NULL, FALSE)）
- 实现：crates/pty/src/windows.rs 新增 clear_console_ctrl_ignore()（手工 FFI，无新依赖；失败路径经 map_ctrl_handler_result 返回 Error::Io，不 unwrap），spawn 创建子进程前调用，失败即 Err 提前返回。
- 测试：windows.rs #[cfg(test)] 2 项——clear_console_ctrl_ignore_returns_ok（本机实测调用返回 Ok）与 ctrl_handler_zero_maps_to_error（0→Err、非零→Ok 映射，覆盖 spawn `?` 分支依赖的失败路径）。cargo test -p pty --lib：2 passed。
- 结果（已验证事实）：run12 i/j 转绿（cmd 与 powershell 的 ping -t 均被中断：^C 回显 + CCCDONE 执行 + TTL 计数停住 1→1）；run13 = 14 PASS/0 FAIL（含 k 与新增 m 正控）；run14 k 红，根因为测试侧竞态（k 的 pre 模式 'PS ' 匹配启动提示符，Ctrl+C 在命令回显前发出），非 app 缺陷；修复（k 的 prePattern 改为命令回显 'Start-Sleep' + pre 后 1s 稳定期）；run15 = 14 PASS/0 FAIL 稳定。
- 结论：假设成立（已验证事实）。X 有效 → 按规格走"实现并保留 + 加测试 + i/j/k 转绿"分支。

### 实验 Y（启动环境侧，不改代码）
- 执行：① schtasks /create /tn tty-y1 /tr "cmd /c wrapper.cmd" /sc once /st 23:59 /it /f + /run（wrapper 内 set APP_DUMP_SNAPSHOT 与 SHELL=powershell.exe 后启动 app）；② explorer.exe 启动 wrapper.cmd。两种均取得交互窗。
- 父进程核验（已验证事实）：m1 父 PID 7896/cmd.exe、m2 父 PID 4028/cmd.exe，均非宿主 shell（宿主 pid 每次不同，记录值 2576）；环境经 wrapper 间接证明（APP_DUMP_SNAPSHOT 写出 = 环境到达子进程；Win32_Process 不暴露环境块，直接读取不可得，如实记录）。
- 测量（已验证事实）：两法 ping -t 与 Start-Sleep 均被 Ctrl+C 中断（marker=True、TTL 停住；cmd 壳网格另见 ^C 回显）。
- 边界说明：X 已有效，Y 在 X 生效代码上运行，非判别性对照（规格中 Y 仅在 X 无效时承担判别）；"无 X 时干净父是否免疫"未测（X 有效使该问题非必要）。
- 过程修正（如实记录）：Y 首跑 wrapper 漏 set SHELL，app 默认起 cmd.exe，Start-Sleep 报 not recognized，Y2DONE 为假阳性；修正后重测。

### 判定
- X 有效 → 实现保留 + 测试 + e2e i/j/k/l/m 转绿（run15 14/14）；Y 已按规格执行并记录。

## 2. 任务2：pwsh 7 FailFast 层级隔离（只记录）

- 环境（已验证事实）：pwsh 7.6.6 @ C:\Program Files\PowerShell\7\pwsh.exe；powershell.exe 5.1 @ C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe；OS = Microsoft Windows 10 企业版 LTSC 10.0.19044（Get-CimInstance 实测）。
- 探针：vt-verify-win/probe_pwsh（portable-pty 0.8.1 openpty 120x30；原始字节收集 + hex + ESC[6n 检测 + 可选回复 + FailFast 模式检测 + try_wait 有界退出；PROBE_REPLY=1 回复 ESC[1;1R）。
- 矩阵（已验证事实，一行一条）：
  1. pwsh.exe 经 ConPTY（带 profile）：failfast=true（输出含 totalWidth/Process terminated/FailFast），exit code 3221225786 (0xC000013A)，1370 字节；字节流含 banner（ESC[2J ESC[m ESC[H "PowerShell 7.6.6"）+ OSC 0;C:\Program Files\PowerShell\7\pwsh.exe BEL + ESC[?25h 后崩溃。
  2. pwsh.exe -NoProfile 经 ConPTY：同崩（1366 字节，标志同）→ 与 profile 无关。
  3. powershell.exe 经同一 ConPTY 参数：无 failfast，10s 窗口存活（killed），400 字节 → 同参数下 PS 5.1 正常。
  4. pwsh.exe 经管道（非 ConPTY）：带 profile 与 -NoProfile 均正常（PIPE-OK/PROFILE-OK，rc=0）→ pwsh 本身非必崩。
  5. 两 pwsh 变体 esc6n_seen 均为 false → 启动查询未在崩溃前发出，PROBE_REPLY 变量未生效（replied=false），如实记录为未触发而非"回复无效"。
- 层级判断（工程判断，依据事实 1-4）：崩溃发生在 **pwsh 自身层**（其输出自带 FailFast 文本 totalWidth=-17），触发条件 = 经 ConPTY（portable-pty 0.8.1）启动；非 app 层（探针不经 app 窗口与渲染）；非 portable-pty ConPTY 创建本身（同参数 powershell.exe 正常）；非 pwsh 通用（管道模式正常）。
- ConPTY 创建参数登记（便携-pty 0.8.1 源码 win/psuedocon.rs:86 实测）：CreatePseudoConsole 传入 flags = PSEUDOCONSOLE_RESIZE_QUIRK(0x2) | PSEUDOCONSOLE_WIN32_INPUT_MODE(0x4)。若上游定位指向创建参数，此为变更点；本阶段不换库（规则）。

## 3. 未解决问题

- pwsh 7.6.6 + ConPTY FailFast：上游 pwsh/ConPTY 交互问题（观察项，非阻塞；windows.toml 示例 shell 在 pwsh 修复前于本机启动即崩，字体项本身无错——banner 已经网格渲染）。
- ConPTY 主端 reader EOF 延迟（约 83s，阶段1 W5 数据）依旧；close 契约顺序已按此设计。
- 上一轮 PHASE2A-WIN.md §4 未解决 1（\x03 不中断非 stdin 型进程）已被实验 X 解决：ping -t 现可中断（run12/13/15 i/j 绿 + Y 双法测量绿）；本文件取代该条。

## 4. 手动清单

- 无新增（本补充任务无需本机手动项；HiDPI/pwsh 观感项仍见 PHASE2A-WIN.md §7）。

## 5. 交付物与证据

- crates/pty/src/windows.rs（X 实现 + 2 测试）；scripts/windows/e2e/run_all.ps1（case m、prePattern 参数化、k pre 修复）；vt-verify-win/probe_pwsh/（矩阵探针）；vt-verify-win/run_y_experiment.ps1、run_ctrlc_realconsole.ps1 / run_ctrlc_realconsole2.ps1（Y 与对照尝试）；vt-verify-win/logs/（e2e-run12..15.log、probe_pwsh_matrix.log、phase2a-win/、e2e-probe/ y1/y2 dump 与 trace）。
- WIN-NOTES 同步：决策（X 保留）、失败尝试（真实控制台对照两 API 路径不可用：GenerateConsoleCtrlEvent 跨控制台无效、WriteConsoleInput 句柄 err=6；Y 首跑 SHELL 漏设假阳性）、已验证事实（pwsh 矩阵 + ConPTY flags）、未解决问题更新（Ctrl-C 条改为 X 已解决）。

## 6. 推送核对（push 后补记）

- 提交：b9e365e223ac73e4e945412366b14e62cc8aa73d（windows: Ctrl-C 继承忽略标志清除 + e2e i/j/k/m 转绿 + pwsh FailFast 层级隔离；含 PHASE2A-WIN-ADD.md 首版）
- git push origin win：319f438..b9e365e win -> win（成功）
- git ls-remote origin refs/heads/win：b9e365e223ac73e4e945412366b14e62cc8aa73d，与本地 git rev-parse win 一致（2026-11-11 本机核对）
