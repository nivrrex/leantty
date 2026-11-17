# WIN-NOTES：Windows 侧决策、失败与请求记录

分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。

## 决策与否决方案

- 决策：构建脚本 scripts/windows/build.ps1 只接收参数（Zig/GhosttySourceDir/ZigSystemDir），仓库内无绝对路径；ZIG 实参经 scripts/windows/zig-inject-target.cmd 包装（无日志版），仅在 `zig build` 且无 -Dtarget 时注入 -Dtarget=x86_64-windows-gnu。已验证事实：注入后 zig 构建参数与 Linux 段一致；构建通过。
- 决策：.cargo/config.toml [target.x86_64-pc-windows-gnu] 仅放 rustflags=["-C","link-arg=-Wl,--no-gc-sections"]，不保留链接器包装器。已验证事实：仅 rustflags 的整工作区构建通过（T1 实测，含 app bin 链接）；验证期受控重放证明 --gc-sections 与 zig COFF 对象 ___chkstk_ms 引用交互致解析失败（证据 C:\vt\logs\w3-*，rp-B1/B3/B6 重放）。
- 决策：WindowsPty::close 先 drop writer 与 master 再 kill+wait 子进程；读线程 join 经 channel+recv_timeout(5s) 有界化（join_thread_timeout），禁止无界 join。依据：Pty trait 模块契约 + Windows 验证期 W5 实测（先等子进程退出会使主端 EOF 延迟约 83s）。
- 决策：spawn 时按 shell 文件名置 UTF-8 代码页——cmd.exe 加 /K chcp 65001，powershell.exe 加 -NoLogo -NoExit -Command chcp 65001。已验证事实：两壳 echo 你好 均入网格且汉字占 2 格（pty_windows.rs t3 测试）；默认 936 下 UTF-8 字节按 GBK 解码乱码（验证期 W5 实录）。
- 决策：shell 传空串时回落 cmd.exe（任务契约"默认 cmd.exe"）。app 层 SHELL 环境变量未设置时 main.rs 回退 /bin/sh（见下方请求栏）。
- 决策：Relative shell 名经 WindowsPty::resolve_program 自解析（PATH 遍历 + POSIX 形态条目转 Windows 形态 + PATHEXT），不动 Pty trait、不改 portable-pty。已验证事实：app 由 bash 直启时 portable-pty search_path 解析失败（CreateProcessW 收到 C:\dev\tty\cmd.exe 拼接路径，os error 2）；resolve_program 后相对名 cmd.exe 与绝对路径 powershell.exe 双变体均起窗成功（MainWindowTitle=tty，Responding=True）。
- 决策（2026-11-11 补遗）：windows.rs spawn 前 clear_console_ctrl_ignore()（SetConsoleCtrlHandler(NULL, FALSE)，手工 FFI 无新依赖，失败返 Error）保留；依据=实验 X 实测：清除后 e2e i/j/k 转绿（ping -t 被中断、^C 回显 + CCCDONE + TTL 停住），继承“忽略 Ctrl-C”标志假设成立（判定与矩阵见 PHASE2A-WIN-ADD.md）。
- 决策：Windows e2e 键盘注入用 keybd_event（SendInput 本机零投递，probe15 交替对照实证）；CJK 用例改为子进程输出 CJK（keybd_event 无法注入 Unicode，渲染断言不变）；SetConsoleCtrlHandler(NULL,FALSE) 变体判为 moot（app 未注册 handler、其控制台窗非输入目标）未实现。

## 失败尝试与原因（已验证事实）

- T4 Ctrl-C（\x03 经主端写入）：cmd + ping -t 单发/连写两次/间隔双写/\x03\r/\x1c(Ctrl+Break)、powershell ping -t、powershell Test-Connection、cmd timeout /T 99 全未中断；同路径 \x03 可中断读 stdin 的 sort（外壳随后可执行 echo）。 portable-pty 写入路径无用户态缓冲（master 写端为 filedescriptor::FileDescriptor 直写管道，源码 win/conpty.rs take_writer）；KeyEncoder Ctrl+C → \x03 映射在 Windows 同库同字节（测试确认）。FFI 机制验证：AttachConsole(子 PID)=0（ConPTY 控制台拒绝外部附加），GenerateConsoleCtrlEvent 虽返回 1 但 ping 未中断。结论：\x03 字节路径对非 stdin 型前台进程无效，原因未定位（待验证假设：ConPTY 行规范仅把 0x03 译作事件交付读方）。
- T7 render 像素测试失败：共享测试 render_pixels.rs 断言 fb[..16*16] 为左格背景区，但帧缓冲为 32 宽行主序，该切片实为前 8 整行（含右格字形像素）；探针帧图（vt-verify-win/logs/probe-render6.log）显示左格零红像素、X 完整落在右格 16-23 列。Linux 通过系字体度量侥幸（X 位图起点在第 8 行以下）。Consolas 度量实测：X 位图 8x9、xmin=0、自第 0 行起绘制。修复属共享测试逻辑，非 cfg(windows) 范围 → 请求栏。
- 探针工程路径过长：vt-verify-win/probe_render/target 全路径触发 zig translate-c/WriteFile 失败（MAX_PATH 类）；改 E:\dev\build\probe-t 后通过。已验证事实。
- git-bash 的 timeout 无法终止 MinGW 子进程（忽略 SIGTERM）；清理须 taskkill/Stop-Process。已验证事实（验证期 W5 遗留）。
- SendInput 本机零投递：返回 2（接受）但事件零到达 app；同运行内 keybd_event 对照键到达且 cmd 回显（probe15）；e2e 注入层改 keybd_event。
- CJK 剪贴板 Ctrl+V 方案废弃：终端无粘贴语义，Ctrl+V 被写为 \x16（case-c dump 见 echo^V）；WM_CHAR PostMessage 亦不被 winit 0.30 投递（probe6-8，真窗焦点确认后仍零事件）。
- 早先"本机无 Microsoft YaHei/YaHei Mono"记录作废：fontdb 0.21 同库探针实测两族均存在且 has_han=true（vt-verify-win/probe_font）；泛型 Monospace→Courier New、SansSerif→Arial（均无汉字）。
- pwsh 7.6.6 本机 ConPTY 启动即 FailFast（totalWidth=-17；80x24 与 120x30 同），powershell.exe 5.1 正常；windows.toml 示例 shell 在 pwsh 修复前于本机启动即崩（字体项本身无错，banner 已经网格渲染）。
- Get-Process MainWindowHandle 命中 6x6 隐藏 "Winit Thread Event Target" 工具窗；真窗须按 class "Window Class"（标题 tty）EnumWindows 枚举；同桌面 PI agent 同名 "tty" 窗，pid 为唯一权威。
- app 为 console 子系统，自带可见 ConsoleWindowClass 控制台窗（抢焦点）；e2e 启动后隐藏。
- ConPTY spawn 后 app 的 fd 2 失效（其后 eprintln 全部丢失）；诊断改用 LEANTTY_TRACE_FILE 文件直写。
- 前台解锁配方（probe13/15 实证）：SPI_SETFOREGROUNDLOCKTIMEOUT=0 + AttachThreadInput(app UI 线程) + SetFocus + SetForegroundWindow + GetForegroundWindow 非零校验。
- 真实控制台 Ctrl+C 对照两 API 路径不可用（probe18/19）：GenerateConsoleCtrlEvent 跨控制台投递无效（ping/sleep 均存活）、WriteConsoleInput 报 err=6（ERROR_INVALID_HANDLE）；判别改由 e2e case m（PS 引擎可中断命令正控）在同一链路完成。
- Y 首跑 wrapper 漏 set SHELL，app 默认起 cmd.exe，Start-Sleep 报 not recognized、Y2DONE 假阳性；修正重测（PHASE2A-WIN-ADD §1）。

## 阶段2A 补充实测（2026-11-11，PHASE2A-WIN-ADD）

- pwsh 7.6.6 经 ConPTY（portable-pty 0.8.1，120x30）FailFast：输出自带 totalWidth=-17/Process terminated 文本，exit 3221225786 (0xC000013A)；-NoProfile 同崩；powershell.exe 5.1 同参数正常；pwsh 管道模式（非 ConPTY）正常；崩溃前无 ESC[6n（回复变量未触发）。层级判断：pwsh 自身层，触发条件=经 ConPTY 启动；非 app/非 portable-pty 创建本身/非 pwsh 通用。
- portable-pty 0.8.1 ConPTY 创建 flags（win/psuedocon.rs:86 源码实测）：CreatePseudoConsole 传 PSEUDOCONSOLE_RESIZE_QUIRK(0x2) | PSEUDOCONSOLE_WIN32_INPUT_MODE(0x4)。

## 请求（共享代码，交 Linux 侧）

- 请求 1：crates/app/src/main.rs 的 SHELL 未设置时回退 "/bin/sh" 在 Windows 无效；请按平台回落（Windows: cmd.exe）。 Windows 侧以 SHELL 环境变量绕过，未擅改。
- 请求 2：crates/render/tests/render_pixels.rs 背景格断言按连续切片 fb[..16*16] 取左格，在行主序宽帧缓冲上取错区域（前 8 整行）；请改为按 (row*width+col) 索引取左格像素。Windows Consolas 下暴露，Linux 侥幸通过。
- 请求 3（观察项，非阻塞）：render blit 仅裁 px>=cell_x+cell_w 与 py>=cell_y+cell_h，未裁 px>=cell_x 与 py>=cell_y；Consolas 当前度量未触发越界，但负 xmin/ymin 字形（斜体/全宽等）可能侵入邻格。
- 请求 4：crates/config 测试 path_priority_cli_env_default 断言期望路径含 Unix /tmp 前缀（XDG 假设），Windows 必败；请共享测试按平台构造期望路径。
- 请求 5（观察项，非阻塞）：pwsh 7.6.6 + ConPTY FailFast totalWidth=-17（本机实测）；examples/windows.toml 的 shell=pwsh.exe 在 pwsh 修复前于本机启动即崩。2B 复验补强（已验证事实）：flags 0/1/2/4/6/7 × 尺寸 80x24/120x30/200x50 共 18 组合全部 exit 0xC000013A；INHERIT_CURSOR(0x1) 只把崩溃推迟到 ESC[6n 应答之后；powershell.exe 5.1 同 harness 不崩；pwsh 不经 ConPTY 正常。结论= pwsh 侧缺陷，改 portable-pty flags 无解，不建议改库。
- 请求 7（2D 复验）：crates/config/tests/config.rs 的 path_priority_cli_env_default 在 Windows 仍失败——测试设置 XDG_CONFIG_HOME 后断言 default_config_path() 落在其下，但 Windows 实现走 APPDATA。实际断言：path "C:\\Users\\Admin\\AppData\\Roaming\\config-<hash>\\config-<hash>.toml" should start with "C:\\Users\\Admin\\AppData\\Local\\Temp\\xdg-test"。Linux 侧只修了 temp 目录构造，请按平台分支或跳过该断言。状态=已验证事实，未擅改（共享测试）。
- 事实补充（2D，已验证）：ConPTY 把控制台宽度处的折行**物化为 CRLF**（probe_ptyin：120 列下 200 个 A 到达为 A*120 CRLF A*80），故 2C.1 的软折行拼接在 Windows 经真实 shell 不可触发；功能在 vt 层正常（probe_wrap 对同字节序列报 wrapped=true）。
- 事实补充（2D，已验证）：请求 6 的 1MB 粘贴排空缺陷已被 2D.2 修复在 Windows 生效——静止粘贴精确 1048576 字节（约 7s），无需额外事件；请求 6 可关闭。
- 事实变更（2D，已验证）：PHASE2B-WIN 记录的 pwsh 7.6.6 ConPTY FailFast 本轮**未复现**：flags 0/1/2/4/6/7 × 80x24/120x30/200x50 共 18 组合全部存活且无 totalWidth；app 以 shell=pwsh.exe 启动正常。2B 与本轮之间仅发生一次本机重启（2B 期间另有多个残留 app/ConPTY 实例）。待验证假设：崩溃依赖会话级 conhost 状态。未据此改任何 spawn 参数。
- 请求 8（3A 评估项，不实施）：ConPTY 版本对照实测（WezTerm 20240203 的 conpty.dll/OpenConsole.exe sideload 到 app.exe 同目）——新版不再把折行物化为 CRLF（PTY 80 列下 200 个 A 到达为单段 200 字节；app 复制出的 A 串最大连续长度 118→198），因此 2C.1 的软折行拼接在 Windows 变为可触发；括号粘贴标记与 SGR 鼠标序列仍被输入通道吞掉（子进程仍只收到 AB / 0 字节），无改善；pwsh 两版本轮都不崩，无法归因。portable-pty 0.8.1 源码（src/win/psuedocon.rs:44-61）已优先加载程序目录的 conpty.dll（shared_library→LoadLibraryW 标准搜索序），故 sideload 零代码改动。若集成：随附两个二进制即可；许可与再分发条款未核实（本机 WezTerm 目录无 LICENSE），且需注意程序目录优先的 DLL 预加载风险。状态=已验证事实+待核实项。
- 请求 9（3A）：crates/pty/tests/pty_windows.rs 的 ctrl_c_probe_powershell_ping 在本机稳定失败——drain_quiet 的 800 ms 静默阈值小于 ping 的 TTL 间隔（~1 s）与本机 powershell 冷启动（~1167 ms），预取只拿到 banner 后断言失败，并使测试进程以 0xC000013A 退出；`crates/pty` 在 3A 合并中无改动（git diff --stat 为空），串行执行同样失败。建议静默阈值提到 ≥1.5 s 或改为等首个 TTL/超时。状态=已验证事实，未擅改。
- 请求 10（3A）：crates/app/src/lib.rs:912 的 `let t_present = Instant::now();` 仅在 `#[cfg(debug_assertions)]` 分支使用，release 构建报 `unused variable: t_present` 警告（clippy dev profile 看不到）；建议加 cfg 门控或 `let _ = t_present;`。状态=已验证事实。
- 请求 11（3C，观测增强）：启动分段仍缺两个打点——renderer/字形缓存初始化完成、首帧提交前。本机实测（release，5 次中位）：config 25 / window 107 / surface 12 / pty_spawn 9 / fontdb 53 / 字体组 158 / first_frame 823，覆盖约 364 ms，还剩约 460 ms 无打点（且 first_pty_byte 1061 晚于 first_frame 823，说明不是等 shell）。不改变行为。状态=已验证事实 + 待拆分。
- 请求 12（3C，待验证）：大段输出在 app 内推进极慢——debug 二进制下 app 内 cmd 执行 `type` 20 000 行（1.28 MB）180 s 内未到结束标记，dump 停在 L000050；100 行 60 s 内未完成；同时段 LEANTTY_PERF_FRAMES 显示 render_ms 中位 1–3 ms（release）/16–18 ms（debug）、present 1–2 ms、coalesced≈130，渲染侧空闲。建议在读线程→channel→vt.feed 路径统计每秒喂入字节数，确认是否存在节流。未定位，仅现象记录。
- 事实变更（3C，已验证）：上一轮把 `ctrl_c_probe_powershell_ping` 的 0xC000013A 归给 clear_console_ctrl_ignore **是错的**。A/B 对照（启用/禁用测试内 reignore）均 5/5 通过；真因是 `t4_e8_ffi_ctrl_c_to_ping` 在 AttachConsole 失败后调 `GenerateConsoleCtrlEvent(0, 0)`，进程组 0 广播到调用者自身控制台组，把 harness 杀掉。已在该测试内修（仅测试进程重新忽略 Ctrl-C），windows.rs 未改。
- 请求 13（3D，P1 数据丢失，已验证事实）：`crates/app/src/lib.rs:1233` 的 `WindowEvent::Resized` 未过滤最小化/零尺寸——最小化时窗口收到 1×1，行列被 `min(1)` 钳位后 `vt.resize(1,1)` 把终端内容裁掉；Windows 实测最小化期间 dump 头为 `# cols=1 rows=1`，恢复后文本丢失（暗像素 4465→2104，逐像素 diff 5413，基线噪声 0；`LEANTTY_FORCE_FULL_REDRAW=1` 同样不救）。建议 cols/rows 为 0（或 physical_size 任一维为 0）时跳过 vt/pty resize，或记住最小化前尺寸。未改共用代码。
- 请求 14（3D，输入侧现象，待验证）：WezTerm conpty（20240203）下向原始读端粘贴 >450 KB 停在约 450 000 字节不再前进（三次：450554 / 456442 / 459002），系统 conhost 正常完成 1 MB（5.7 s）与 10 MB（60.7 s）；release 构建。是否为 ConPTY 输入流控或探针（`ReadFile` 原始读）在新 ConPTY 下的读取差异未区分。建议在共享读线程→channel→vt.feed 路径加“每秒喂入字节数”观测。
- 事实更正（3D，已验证）：3C 记录的“1 MB 级输出推进极慢而渲染空闲”在 release 下**不成立**——20 000 行 5.47 s、2 000 000 行 330 s（中位）均正常完成，渲染 render 4 ms / present 1 ms。3C 当时用的是 debug 构建 + 每帧写 dump，属测量方法污染；该项降级为测量问题，不是缺陷。
- 事实（3D，已验证）：release 2M 行 120×30 三次 323.6/333.7/330.1 s；200×60 单次 325.0 s（render 中位 11 ms）；WezTerm conpty 120×30 单次 229.0 s（比系统 conhost 快约 1.44×）。
- 请求 15（3D，观测增强）：启动剩余约 420 ms 无打点（默认配置 config 27 + window 62 + surface 1 + spawn 10 + fontdb 44 + 字体组 136 = 280 ms vs first_frame 701 ms）；外部实测窗口可见时刻距 first_frame 仅 7–116 ms，说明缺口在进程启动到窗口可见之间（winit 事件循环 + 首帧合成）。建议补 renderer/字形缓存初始化完成、首帧提交前两个打点。
- 请求 16（3E，观测增强，已验证事实）：`create_renderer` 内部仍有约 359 ms 无打点（Windows release，5 次中位：config 26 + window 86 + surface 4 + spawn 10 + fontdb 46 + 字体组 138 = 310 ms，而 renderer_ready 669 ms）；后面的尾部已由 4C 新点拆开（renderer_ready 669 → pre_first_submit 740 → first_frame 764）。建议在字体加载完成后、字形缓存/图集构建后各加一个 `[perf]` 点。
- 事实（3E，已验证）：最小化/恢复守卫有效——3 次最小化期间 dump 头均保持 120×30（3D 曾为 1×1 并丢内容），恢复后逐像素 diff=0（基线噪声 0）；最大化→还原也 diff=0（最大化期间 240×63 属正常）。
- 事实（3E，已验证）：反色同 case34 阈值（≥768）：默认反色高亮像素 2768、彩色反色红像素 1280、选区 diff 2176；但本机 CPU 渲染下“白色”实为 (197,200,198)、红块 (204,102,102)（约 0.77 倍），纯 #FFFFFF 计数仅 87——原因未核实（待验证）。
- 事实变更（3E，已验证 + 待验证）：3D 记录的“WezTerm conpty 下 1MB 粘贴停在 ~450KB”本轮**未复现**（2/2 都在 15 s 内完成 1 048 576 字节）；系统 conhost 7.77 s。停滞时的条件差异未定位，3D 与 3E 之间无代码变更（仅合入 4C）。
- 事实（3E，已验证）：ConPTY 输出路径外部参照——WezTerm 终端自身 `type` 同一 2M 行文件 >900 s 未完成，而本 app 在同机器为 330 s（系统 conhost）/229–467 s（WezTerm conpty）；app 侧 feed_bytes_per_s: 442–470 KB/s（系统）/371–387 KB/s（WezTerm），chan_depth_max 均为 1，render 3–4 ms。
- 事实更正（3F-a，已验证）：3E 的 t4 反色"失败"是**判据问题，不是渲染问题**。反色块实测为整块纯色 1280 px（默认色，颜色 #C5C8C6）与 1295 px（彩色，颜色 #CC6666），同 case34 的 ≥768 口径都通过；3E 用“严格 #FFFFFF 计数”才得到 87/0。
- 事实（3F-a，已验证）：`--config` 的 `[colors.primary]` **生效**（#000000/#ffffff 与对照 #003300/#ffff00 均精确命中），但 `[colors.normal]`/`[colors.bright]` **不生效**：两份配置下 `ESC[31m` 都是 #CC6666。机制：`crates/vt/src/terminal.rs:28` 的 `VtTerminal::new(cols, rows)` 不接受 palette、`crates/vt` 无 palette 概念，VT 层用内置默认调色板把 ANSI 色解析成显式 RGB，renderer 的 `cell.fg.unwrap_or(fg_default)` 只会对“VT 未给色”的格子用配置 palette（`crates/render/src/decide.rs`）。反色格底色同样取自 VT 层（#C5C8C6），不是配置前景色。
- 请求 17（3F-a，P2 跨平台）：配置调色板未到达 VT 层——建议把 palette 传入 VT（`Terminal::new`/`set_palette`），或在 renderer 侧对“VT 给出显式 RGB 的 ANSI 索引格”做 palette 覆盖。属共用代码，未修。状态=已验证事实（Windows release 实测）。
- 事实（3F-a，未核实）：#C5C8C6 / #CC6666 在仓库源码中检索不到，判定为编译进 libghostty-vt-sys 的默认调色板取值；具体出处未逐行核实。
- 事实（3F-b，已验证）：4F 的调色板接线在 Windows **生效**——`ESC[31m` 与彩色反色块均为配置的 normal.red（#d32f2f / (211,47,47)），3F-a 时的 (204,102,102) 是合并前的内置 palette 值；primary 背景/前景仍正常。
- 事实（3F-b，缺陷候选，待验证）：**默认色反色块取的是 normal[7]（#757575），不是 primary.foreground**——把 primary.foreground 从 #ffffff 改成 #ffff00 后该块仍是 #757575（10 格 1280 px）。三处口径不一致：屏幕实测 #757575、vt 探针报 cell.fg=None/bg=None、render 单测对同样格子渲出 primary fg。请求 18。
- 事实（3F-b，未核实）：OSC 4/104 在 vt 层单测通过（osc_overrides_then_resets_back_to_configured_palette），但 app 层三次截图都未出现 #AABBCC（31m 仍为配置红），无法区分序列未送达还是未生效；请求 19（OSC 状态观测点）。
- 事实（3F-b，已验证）：4E 放开门控后，Windows 上首次真实运行 background_load(2)/baseline_alignment(1)/cell_metrics(1)/fallback_chain(3)/glyph_clip(1)/preedit_ghost(2)/wide_glyph(4)/inverse_pixels(4) 全通过；无运行时 SKIP，仅 2 个 `#[ignore]` 基准（bench_frame、font_load_time）。DejaVu Sans Mono / Liberation Mono / Noto Sans CJK SC / Consolas / Courier New 在本机均已安装，测试不是靠回退字体勉强通过。
- 事实（3F-b，已验证）：启动新点 cpu_renderer_ready_ms 中位 465.9（5 次 446–561），first_frame 781（751–846）；字体加载结束后到 cpu_renderer_ready 约 362 ms、window 后到 renderer_ready 约 120 ms 仍无打点（请求 20）。
- 事实（3G，已验证，ConPTY 字节取证）：系统 conhost 把 **`ESC[7m` 物化为 `ESC[30m`+`ESC[47m`**（黑前景/白背景），并把 10 个空格改写为 `ESC[10X`/`ESC[10C`；WezTerm conpty 则原样保留 `ESC[7m` 与空格。所以 3F-b 的“默认色反色块 = normal.white”是**平台行为**（palette 索引 0/7），不是 app 缺陷；请求 18 降为文档事项。
- 事实（3G，已验证）：`OSC 4` 在两种 ConPTY 下**都原样到达**（因此 3F-b 的“OSC 4 不生效”不能归因于 ConPTY，原结论证据不足，降为未核实）；**`OSC 104` 在两种 ConPTY 下都被丢弃**（平台限制，改色后无法复位）。
- 事实（3G，已验证）：视口上滚隐藏光标在真实 Windows 上成立——品红光标计数：底部 128 px、上滚 5 格后连续 3 次均 0 px、回底 128 px 且画面与初始逐像素 diff=0。
- 事实（3G，测试工具缺陷）：`mouse_event` 的 `dwData` 传 `0xFFFFFF88` 会被 PowerShell 当 Int32 −120，绑定 uint 参数抛 MethodException，导致“滚轮下滚”静默无效（影响 2B c2a 等旧脚本的下滚步骤，当时未断言方向所以结论不受影响）；正确写法 `[uint32]4294967176`。
- 事实（3H，已验证，已实施）：Windows 上粘贴时主线程停顿——真实路径（右键 Paste + 前台 `ping -t` 不读 stdin）下 `loop_gap_ms` 最大 **1 MB 1104/1114/1111 ms、5 MB 1093/1108/1099 ms**（与数据量无关，取决于前台占用时间）；在 `crates/pty/src/windows.rs` 实现 `take_writer`（把 spawn 时已从 master 取出的写端移交 app 写线程，不改任何 trait 签名）后降到 **55–71 ms**。门禁：1MB 送达 1048576 字节、5MB 粘贴中 Ctrl-C 后仅送达 123130 字节（未写数据丢弃）、关窗 1.6–2.5 s 退出、pty_windows 21/21、回退实现必红（1135/1086 ms）。
- 事实（3H，已验证）：启动缺口拆分（release，第 4 次无抖动运行 1318 ms）——**4 组字体逐面处理 675 ms（51%）**为最大单项，其中 fontdb 扫捾 69 ms（5%）、文件读取 0.6–1.1 ms/组、fontdue 解析 22–30 ms/组，剩余约 100 ms/组无打点；窗口创建 102 ms（8%）；其余各段均 ≤ 112 ms。3E/3F-b 的 ≈362 ms 即字体组总体，≈120 ms 即窗口创建 + ime + 就绪过渡。请求 1。
- 事实（3H，已验证）：`writer::tests::paste_queue_respects_cap` 本机 10 次跑 3 次失败（队列上限断言与写线程消费赛跑），属共享测试时序假设；未改，请求 2。
- 平台限制登记（3G，按要求写入）：系统 conhost 将 `ESC[7m` 物化为 `ESC[30m`+`ESC[47m`（默认色反色块显示 `normal[7]` 而非 `primary.foreground`）；`OSC 104` 被系统与 WezTerm 两种 ConPTY 丢弃（`OSC 4` 改色后无法复位）。
- 事实（3I，已验证）：**OSC 4 在 Windows app 层确实生效**——系统 conhost 与 WezTerm conpty 下 `ESC[41m` 色块都变成 `#AABBCC`（1280 px，配置红 #d32f2f 像素 0），且用 pwsh 脚本首行写标记文件证明注入生效；故 3F-b 的“未核实”转为“已验证（生效）”，请求 19 可关闭。**OSC 104 后仍为 #AABBCC**（两种 ConPTY 一致，与 3G 字节取证吻合）。
- 事实（3I，已验证）：`writer::tests::paste_queue_respects_cap` 不稳定——连续 30 次失败 **5 次（17%）**，消息 `接受量不应远超上限（accepted=1114112）`；`drain_to_writer` 相关 45 次里 1 次失败。请求 1。
- 事实（3I，已验证）：Windows 上 Ctrl-C 取消粘贴 **0/5**（15 s 内标记未出现；debug 等 90 s 仍不可用；100KB 也 0/2），中断后屏幕仍是 `PS C:\Users\Admin> aaaa…`；无粘贴时同一命令 263 ms 生效（对照有效）。已送达的粘贴文本仍在控制台输入缓冲/PSReadLine 行内被逐字消费，Ctrl-C 只能丢弃 app 队列中 **未写出** 的部分（3H：5MB 只送达 123130 字节）。请求 2。
- 事实（3I，已验证）：关窗耗时——无粘贴 390–464 ms（5 次）；5 MB 粘贴中固定 **2438–2467 ms**，stderr 5/5 出现 `pty writer: join 超时（2s），已分离线程（对端长期不读）`；退出后 ping/app 残留均 0。
- 事实（3I，已验证）：字体对照（release 各 5 次）first_frame 中位/最小：A（Consolas+YaHei 回退）1440/1378、B（Consolas 无回退）1336/1245、C（Courier New 无回退）1376/1365；fontdb 三组 37–42 ms。同组内噪声 1–3 倍，只能给量级。
- 事实（3J，已验证）：写线程测试稳定性——`writer::` 连续 30 次 **0 失败**（4J 修复前 5/30，消息 `接受量不应远超上限（accepted=1114112）`）；`follow` 与 `key_matches` 各 10 次均 0 失败；`cargo test --workspace` 40 个目标全绿。
- 事实（3J，已验证）：输入回底——上滚后拇指底 424/480（lastLine 242），键入后回到 **479/480（lastLine 300，marker 1139ms）**；后台输出（ping）与 Copy（Ctrl+Insert）均不移动视口（408→408）。**短粘贴（<1MB）不回底（424 保持），1.2MB 分块粘贴回底（424→479）**——原因是 `paste_text_accepted()` 未调用 `follow_viewport_before_input(Paste)`，而 ≥1MB 路径调了。请求 1（共用，未改）。
- 事实（3J，已验证）：无自定义绑定时默认 Ctrl+Shift+C 3/3 生效——剪贴板 = `CSELECTME`，随后 `echo MARK > 文件` 正常执行且无杂散 `C`（4K 前的问题未复现）。
- 事实（3J，已验证）：粘贴积压拒绝在第 3 次 5MB 时发生，日志 `[perf] paste_rejected bytes=5242880 backlog=7471104 cap=12582912 oversized=false`；RSS 1518→1521 MB 有界；拒绝后 Ctrl-C 仍 15s 内无法执行命令（与 3I 一致）。
- 事实（3J，已验证）：字体面成本（20 组样本）——`font_group_ms` total 中位 168.4/最小 144.7，其中 `face_ms` **恒为 0.00**、read 0.7、parse 23.9、cache 7.2，未归属 **137.0ms（81%）**；墙钟分布 begin→seg 73.7 / seg→cache 59.0 / cache→end 31.7。因此“每组 ~100ms”不在 face_ms。请求 2（观测）。
- 事实（3K，已验证）：官方 ConPTY 可再分发且可核实——NuGet `Microsoft.Windows.Console.ConPTY` 最新稳定版 **1.25.261109001**（2026-11-11，MIT）；x64 `conpty.dll` 119 608B sha256 e971856f…、`OpenConsole.exe` 1 089 336B sha256 b17e48b0…，两者 Authenticode 均 **Valid / CN=Microsoft Corporation**（含时间戳）；包 sha256 3d77f535…；包内不含许可文本（MIT 原文另存）。
- 事实（3K，已验证，三方对照）：与系统 conhost 相比，官方 1.25 **不物化折行**（200 字符到达为单段，S 为 80/80/43+CRLF）、**保留 `ESC[7m`**、且 **`OSC 104` 可达**（S/W 均丢弃）；e2e 13/14 + 11/11 + 6/6 + 8/8（唯一失败为已知注入抖动）、pwsh 20/20、1MB/5MB 粘贴精确、关窗与 loop_gap(86ms) 均无新增失败。代价：启动 first_frame 中位 +110ms、2M 行吞吐 355s vs 330s。判定：满足启用门槛。
- 事实（3K，已验证）：app 空闲内存基线（release，60s 稳定）——默认配置（Consolas+YaHei 回退）WS 878–880MB / Private 896–902MB；**去掉回退后降至 189–190MB（差 ~690MB）**；仅 Courier New 207MB。3J 的 1518MB RSS 是空闲基线加粘贴余量，不是粘贴造成。Linux CPU 基线 ~215MB（PHASE4K-LINUX）。
- 事实（3K，已验证）：字体文件打开成本——consola 首次读字节 0.16ms/全读 0.95ms/热开 0.29ms；msyh.ttc 0.15/9.78/0.34ms；系统路径与临时副本无差异 → 3J 的“137ms 来自首次打开被扫描”假设**不被数据支持**（待验证→否定）。
- 事实（3K，流程事故）：本轮曾按 PID 结束两个 `OpenConsole.exe`（7552/11252），WezTerm 也使用 OpenConsole 作为 pty 宿主，可能导致用户终端会话中断。后续规则：不结束任何 OpenConsole 进程，不论 PID 来源。
- 事实（3L，已验证）：`font_group_us` 的 `total_us − steps_us` 缺口不是字体处理，而是**测试端 stderr 重定向方式**——同一 release 二进制，PowerShell `Start-Process -RedirectStandardError` 下缺口 122–162ms、首帧 1962ms，bash 直接 `2>file` 下缺口 1.3–1.9ms、首帧 418ms；独立探针（probe_logcost，50 行同长度）实测每次 eprintln 到文件：bash 句柄 28µs / PowerShell 重定向 **6754µs** / NUL 4µs。3J 的“137ms 未归属”即此项。
- 事实（3L，已验证）：主字体面（Consolas ×4，bash 重定向）parse 22–27ms、clone 7–10ms、steps 30–47ms/面，四组合计 ~165ms，`total−steps` 0.5–1.9ms。CJK ttc 面（YaHei 作主字体）：19.7MB/16.9MB，read 11–15ms、**parse 490–585ms、clone 161–195ms**；同 face 被后两组复用时纯 clone **135–155ms/组**（无 `font_steps_us` 行）。
- 事实（3L，已验证）：默认配置下后台回退面 YaHei 由 `load_font_list()` 非缓存路径加载，**无 `font_steps_us` 输出**（`bg_fonts count=3`），故默认配置的回退面 parse/clone 不可观测（请求 1）。
- 事实（3L，已验证）：内存时间线（20ms 采样 + app 自身 `since_start` 对齐）——默认配置在 `bg_fonts` 到位（~2.5s）后**连续爬升约 650ms**、峰值 **~1010MB**、随后回落 ~130MB 稳定在 877MB；不是一次性跳升，也不是 4×170MB 台阶；无回退配置停在 189MB，60s 内两者均平坦。
- 事实（3L，已验证）：`scripts/windows/package.ps1` 端到端可用——dist 8 文件 9 093 735B、zip 3 436 974B（sha256 04b55b79…）；hash 不符时非零退出且不留半成品；部署验证在含空格+中文路径、异工作目录启动下，模块列表确认加载的是 dist 内 conpty.dll；删两文件后回退系统 conhost 可正常启动（无 conpty.dll 模块）。
- 事实（3L，已验证）：NOTICE.md（上游 commit f6de9e0，sha256 e7fbaade…）逐字随附；其中 chromium/base/numerics、cmark 要求二进制形式复现声明，interval_tree/PCG/Oklab/cmark/fzf/GSL/UI-XAML/VirtualDesktopUtils/wil 要求所有副本包含版权与许可声明；未发现与再分发冲突的条款（不作法律结论）。
- 事实（3L，未取得结论）：部署验证的像素级折行判别在 120 列窗口下失效（conhost 与 conpty.dll 都按窗口宽度换行，三种布局“最右暗像素”同为 958px），未完成；仅剩 conpty.dll 时的 pty 宿主身份未核实。
- 事实（3L，已验证）：本轮三次 dist 启动/关闭前后，OpenConsole PID 集合（328,9376）与 conhost 数量（25）**完全未变**（采样点为启动前/关闭后 2s，运行期间瞬时宿主未采样）。
- 事实（3M，已验证）：4N 的 Arc 共享在 Windows 生效——默认配置（Consolas ×4 + YaHei 回退）空闲 ws 由 3L 的 877–879MB 降到 **252MB**（priv 244MB），峰值由 ~1010MB 降到 **252MB（无过冲）**；仅 Consolas 基线由 189MB 降到 **83MB**；单 YaHei Regular 实例参考值 220MB（含基线 83），两实例 371MB。爬升段由"650ms 内 +740MB 后回落 130MB"变为"约 350ms 内 +100MB 后稳定"。
- 事实（3M，已验证）：4N 三个新测试在 Windows 真实通过——黄金帧 hash `507485fb7945fb8f` 与 Linux 相同（DejaVu Sans Mono + Noto Sans CJK SC 本机均安装）；`same_face_is_shared_not_cloned` 通过；`measure_font_instance_and_clone_cost` 非 SKIP：Noto CJK 19.5MB ttc 实例 **298.7MB**、clone 再 +298.7MB（1:1 深拷贝），DejaVu 340KB→8.9MB。
- 事实（3M，已验证，修正 3K）：直接句柄重定向下 S（系统 conhost）first_frame 中位 **419ms**/最小 395，N（官方 ConPTY 1.25）**524ms**/416，差 **+105ms**；3K 的 +110ms 结论方向成立，但 3K 记录的绝对首帧 1.6–1.9s 不成立（PowerShell 重定向放大 4.2–4.4 倍：1772ms vs 419ms）。**明确记录：3C–3L 的启动数据均用 PowerShell 重定向采集，绝对值已知偏高，只能横向比较，对照 3L §2.1 与 3M §4.2。**
- 事实（3M，已验证）：只剩 `conpty.dll`（删 `OpenConsole.exe`）时 app 正常启动、无错误输出，但 pty 宿主是 `C:\Windows\system32\conhost.exe --headless …` —— **静默回退系统 conhost**，不是 dist 内 OpenConsole；完整 dist 下宿主是 `<dist>\OpenConsole.exe --headless …`（子进程命令行取证）。
- 事实（3M，已验证）：折行判别的正确判据是原始字节——系统 conhost 把 200 字符物化为 80/80/43+CRLF，官方 1.25 与 WezTerm 均为单段 200 无 CRLF；120 列窗口下像素判别对两者同形，不可用。
- 事实（3M，未核实）：t5 的键入注入未取得有效证据——`terminal.shell.args` 被覆盖为 `/K "chcp 65001"`（子进程命令行为证），合成按键在对照组（完整 dist）也未能写入标记文件，故判为注入无效，输入路径未核实。
- 事实（3N，已验证）：`clippy -D warnings` 在 dev/release 两配置均失败于 **upstream 4O** 的 `crates/render/src/lib.rs:63 'pub(crate) use eperf;'`（unused import），release 构建同样报该警告；按规则未改 `crates/`，记为请求 1。
- 事实（3N，已验证）：`frame_bytes_identical` 在本机 **SKIP**——4O 引入字体指纹校验，本机 `DejaVuSansMono.ttf` 为 340712 B（指纹 `:53dd`），期望 343140 B（`:c69b`）；即使帧哈希仍等于 GOLDEN（507485fb7945fb8f）也按指纹跳过，故该黄金断言在 Windows 上不再执行（请求 2）。
- 事实（3N，已验证）：kit v1 在本机（无 GPU 构建）`-Quick` 5.7 min、默认参数（浸泡 5 min）10.1 min 全程 PASS/N-A 并生成 results zip；P2 因本构建无 GPU 后端自动 N/A（判定依据=`[perf] gpu_init_ms` 是否出现，而非 stderr 文案）；`first_frame_ms` n=5 中位 419 ms、空闲 ws 269.2 MB、吞吐 120×30 63.1 s（feed 374 794 B/s）、200×60 66.8 s（feed 280 308 B/s）、最小化与遮挡对比 diff=0。
- 事实（3N，已验证）：试跑共修 8 个 kit 自身缺陷——ASCII 编码写 `launch-*.cmd` 破坏含中文的路径、shell cwd 不在 kit 目录、`sample.ps1` 绝对路径、吞吐等待时限的运算优先级、吞吐完成标记路径不一致、`[perf] frames` 无 `key=` 形式导致解析失败、GPU 可用性误判（`auto` 变体被当成 GPU）、缩放/最小化截图基线用了过期快照。
- 事实（3N，已验证）：kit 包 `vt-verify-win/tonight-kit-v1.zip`（不入库）3 460 316 B，sha256 45a93931cf187f4377e2852f9992c92f99b7a05f268f8be8c11639fd7e866ae4；内含 12 个文件 9 167 934 B，ConPTY 两文件与 NOTICE.md 的 sha256 与 3K 一致。
- 事实（3O，已验证）：合并 4P 后 Windows release 含 wgpu（`wgpu 30.0.2`，特性 dx12+vulkan+wgsl），构建 222 s、**零告警**，app.exe 7 866 405 B → **18 084 829 B（+10.2 MB）**；clippy 两配置 0（3N 的 `unused import: eperf` 因 4P GPU 路径实际使用而消失）；`cargo test --workspace` **196 passed / 0 failed**，GPU 用例在本机全部 SKIP 并打印"无可用真实 GPU 适配器（仅软件实现可用）"。
- 事实（3O，已验证）：无 GPU 机器上 `backend="gpu"`（默认/dx12/vulkan）**exit=1、0.7–1.5 s、无 panic 无挂起**，错误原文 `create_renderer(gpu): render error: backend error: 未找到真实 GPU 适配器（仅软件实现可用）`；`auto` 走 CPU（`[perf] backend=auto → cpu`，无 `gpu_init_ms`），窗口正常 ws 269 MB；`gpu_adapters` 仅枚举到 `Microsoft Basic Render Driver … type=Cpu`。
- 事实（3O，已验证）：CPU 路径无回归——`first_frame_ms` 10 次中位 414/最小 320 ms（3M 419/395）、`run_all` 14/14、`run_all` 外的 clip 11/11、mouse 6/6、2C 8/8；1MB/5MB 粘贴精确、Ctrl-C 65535 级别不变（65 531）、`loop_gap` 97 ms、N 条件折行单段 200。默认配置空闲 ws **268–270 MB**（3M 252）且启动期出现 **406–416 MB 瞬时过冲**（3M 无），只记录未归因。
- 事实（3O，已验证）：`crates/pty/src/windows.rs` 新增成对部署自检——目录里有 `conpty.dll` 但无 `OpenConsole.exe` 时打印默认可见警告（不改变行为），实测在只放 dll 的目录出现该行、完整成对目录 0 行；配套单测 `conpty_pair_detection_needs_both_files` 通过。
- 事实（3O，已验证）：kit v2（含 GPU 构建）在本机 `-Quick` 5.4 min、默认 10.1 min 全 PASS/N-A，P2 对三变体分别给出 N/A 原因（auto 按设计走 CPU / dx12 与 vulkan 无可用适配器）；包 `tonight-kit-v2.zip` 6 618 947 B，sha256 98e4c539…，12 文件 19 390 489 B。
- 事实（3P，已验证）：`WGPU_BACKEND` 在本构建**完全无效**——四种传法（`set A=v &&`、`set A=v&&`、`set "A=v"`、PowerShell 环境块经 cmd）日志一律 `backends=Backends(VULKAN | DX12)`；探针 `probe_wgpuenv`（同版本 wgpu 30.0.2）实测 `Backends::from_env()` 对 dx12/vulkan/vulkan,dx12/DX12/"dx12 " **恒返回 None**，而 `std::env::var` 能读到值；源码 `wgpu-types-30.0.2/src/backend.rs:156` 经 `env.rs` 依赖 `std` feature，而本项目 `default-features = false` 未启 `std`（`cargo tree -e features` 无 `wgpu-types feature "std"`）。**不是 kit 的尾随空格问题**（请求 1）。
- 事实（3P，已验证）：目标机 P3 超时的直接原因是 3O 版固定 540 s 时限；kit v3 改为 `后端数 × SoakMinutes × 60 + 180`（本机 `-Quick` 实测标题 `soak 1 backend(s) x 1 min (limit 240s)`，默认 480 s，2 后端默认 780 s）。
- 事实（3P，已验证）：P4 的 zip 已改为流式 `ZipArchive` → `.partial` → 原子改名，>50 MB 文件只存末尾 5 MB（实测 60 MB → 条目 5 242 880 B），逐文件容错，主路径失败产出最小 zip（`-ForceZipFailure` 实测 84 473 B、24 文件、含 summary.md、可解压）。
- 事实（3P，已验证）：P4 OOM 的四个候选中，**"zip 自包含"被证伪**（zip 在 RunDir 之外）、**"整文件 ReadAllText"被证伪**（3O 代码无此调用）、**"遗留 app 持日志"被复现**（独占句柄下两种策略都报共享冲突，已由 P4 前 `Stop-AllKitApps` 消除前置条件）、**"Compress-Archive 内存"机制成立**（154.5 MB 输入 → 峰值 WS 355 MB ≈ 1.7×，Optimal 4.2 s）；目标机 1029 s 的 OOM 未在本机复现（本机 run 目录约 1 MB）。
- 事实（3P，已验证）：DPI 感知必须在新进程第一条语句设置——本机实测全新进程调用 `SetProcessDpiAwarenessContext(-4)` 返回 True，加载 `System.Windows.Forms` 后再调用返回 False；kit v3 用子进程 `shot.ps1` 承担枚举与截图，`summary.json` 实测 `dpi_aware=true`；本机缩放为 96dpi/1.0（非 150%），150% 效果未验证。
- 事实（3Q，已验证）：4Q 的严格解析在 Windows 上按预期工作——`WGPU_BACKEND` 取值 dx12/DX12/"dx12 "/vulkan/"dx12,vulkan"/vk/空/乱码 九种，`backends_env` 原样回显（含尾随空格），`backends_effective` 大小写无关、空格裁掉、组合正确；无法识别的记号直接报错并回显（`WGPU_BACKEND='dx' 含无法识别的后端记号 ["dx"]`）；空值等价于未设置。DX12 枚举到 `Microsoft Basic Render Driver type=Cpu`（被过滤），Vulkan 枚举 0 个。
- 事实（3Q，已验证）：`LEANTTY_GPU_ALLOW_SOFTWARE=1` 使 GPU 测试从 SKIP 变为真实执行——`gpu_parity` 8/8 通过（199.9 s，WARP）；默认无开关时全部 SKIP。
- 事实（3Q，已验证，WARP/DX12 软件路径）：启动 10 次 first_frame 中位 477/最小 395 ms，`gpu_init_ms=107.6`，adapter=`Microsoft Basic Render Driver` `software=true`，surface=`Bgra8Unorm/Fifo/Opaque 960x480`，用户可见软件适配器警告 1 行；内容与 CPU 同图 max_diff=252、diff_ratio 2.22%、gt32 1.46%、结构性缺行/列 0/12（12=滚动条）；**空闲 20 秒 frames_window/os/resize/other 全 0 → 4Q 的"空闲不渲染"修复在 Windows 上也成立**；20 次缩放 ratio 0.28%、最小化与遮挡 diff 0；吞吐 30 万行 128.4 s（CPU 49–62 s）；1MB/5MB 粘贴精确、Ctrl-C 65 531、loop_gap 39 ms；浸泡 5 min ws 316.6 MB、priv 541.3 MB 无趋势；关窗 236 ms 无 panic。
- 事实（3Q，已验证）：kit v4 的 P2 在无真实适配器时会跑 warp 兜底变体并给 `PASS (WARP only)`；两次试跑（-Quick 5.8 min、默认 17.9 min）P2 均真实执行 WARP，`idle 20 s -> 0 frame(s)`、`vs S_cpu diff_ratio 0.83%`、`throughput 59.5 s`。
- 事实（3Q，已验证，kit 自身 7 个缺陷）：P2 缺 warp 兜底；Confirm-Typing 只吃 session；主进程内 `System.IO.Compression` 与已加载的 WinForms 混用会卡死（拆 `zip-run.ps1`）；`& powershell.exe @args` 用自动变量 `$args` 导致子进程变交互式挂起（改 `$zipArgs`）；`summary.json` 序列化整个 Env 图会卡死（只序列化原语）；stage 记录字段名是 error 不是 note；`@(...)|Where` 单元素非数组导致 `.Count` 报错。
- 请求 1（3Q，观测 P3）：`gpu_surface_ms` 之后建议补一条 present/帧延迟配置的打点，便于区分 WARP 与真实 GPU 的 present 行为。状态=已验证事实（缺打点）。
- 请求 1（3P，构建配置 P1）：`crates/render/Cargo.toml` 的 wgpu `default-features = false` 未启 `std`，使 `WGPU_BACKEND` 在编译期被丢弃（4P 文档宣称可用）。建议加 `"std"`；否则 Windows 上的后端选择与其观测永远无效。状态=已验证事实（源码+探针）。
- 请求 1（3O，观测 P3）：启动期内存峰值（406–416 MB）在 `LEANTTY_PERF_FRAMES=1` 下没有可对齐的分段打点，目标机上若要定位需在 renderer 初始化前后补采样点。状态=已验证事实。
- 请求 1（3N，CI P1）：`crates/render/src/lib.rs:63` 的 `pub(crate) use eperf;` unused import，使 `clippy -D warnings` 与 release 零告警门槛无法达成；删掉或使用即可。状态=已验证事实。
- 请求 2（3N，测试 P2）：`frame_bytes_identical` 的 `GOLDEN_FINGERPRINT` 绑定单一 DejaVu 文件尺寸（343140 B），Windows 侧为 340712 B 因而恒 SKIP，黄金校验在该平台失效；建议改为指纹集合或分平台期望值。状态=已验证事实。
- 请求 1（3M，功能 P2）：`render::load_font_list()`（后台回退路径）不打 `font_steps_us`（3L 提出，3M 用 YS/Y1 兜出数量级）。状态=已验证事实。
- 请求 2（3M，功能 P2）：`terminal.shell.args` 在 Windows 上被丢弃——子进程命令行恒为 `cmd.exe /K "chcp 65001"`，配置的 args 不出现；若属有意（强制 UTF-8）需在文档/校验处明示，否则应改为追加。状态=已验证事实（命令行证据）。
- 请求 3（3M，观测增强 P2）：只剩 `conpty.dll` 时静默回退系统 conhost 且无任何日志，建议 `load_conpty()` 成功但宿主不是同目录 `OpenConsole.exe` 时打 warn（不改行为）。状态=已验证事实。
- 请求 1（3L，观测增强 P2）：`render::load_font_list()`（后台回退路径）不打 `font_steps_us`，默认配置下回退面 YaHei 的 parse/clone 完全不可观测；建议该路径复用 `load_font_with_fallback_cached` 的观测或在 `add_background_fonts` 处补点。状态=已验证事实（`bg_fonts count=3` 且无 font_steps_us 行）。
- 请求 2（3L，观测增强 P2）：`font_group_us` 缺 `cache_hit` 字段——本轮 group2/3 无 `font_steps_us` 行却有 135–155ms `load_us`，只能靠“缺行”反推为缓存命中的 `Font::clone()`；建议显式标注。状态=已验证事实。
- 请求 3（3L，测试脚手架，不改产品代码）：perf 采集必须用直接文件句柄重定向——PowerShell `Start-Process -RedirectStandardError` 每次写 ~6.8ms，把 `total−steps` 与首帧放大 4–5 倍（1962ms vs 418ms），跨平台/跨轮数据不可比。状态=已验证事实。
- 请求 6（P1，功能完整性）：app 的 1MB 级粘贴排空依赖事件循环唤醒——about_to_wait 每轮只写一个 pending 块且不自触发下一轮，无新事件时排空停摆（Windows 实测 131066/1048576；持续产生鼠标事件后全部送达）。建议 push 后 request_redraw() 或单轮按预算循环排空。未擅改（共享行为变更）。
- 事实补充（非请求）：ConPTY 输入通道吞掉 ESC[200~/ESC[201~ 与 SGR 鼠标序列（probe_ptyin：写 ESC[200~AB ESC[201~ 只到 AB；写 ESC[<0;10;5M 到 0 字节），故括号粘贴与 SGR 上报在 Windows 子侧不可观测，仅能由 Linux e2e 覆盖。

## 已修正的共享代码（最小、cfg(windows) 范围或门控，登记）

- crates/pty/src/lib.rs：加 #[cfg(windows)] pub use windows::WindowsPty as NativePty;（app 以 NativePty 引用，Windows 编译所需）。
- crates/pty/tests/pty_unix.rs：加 #![cfg(unix)] 门控（/bin/sh 测试在 Windows 不得编译运行）。
- crates/app/src/clipboard.rs：arboard 3.7 的 X11 扩展 API（GetExtLinux/SetExtLinux/LinuxClipboardKind，仅 unix）致 Windows 编译失败（E0432/E0433）；最小拆分为 #[cfg(unix)] use + primary_get/primary_set 双实现（Windows 返 Err；Primary 实际行为实测记录于 PHASE2B-WIN.md）。E0308 修正（Error::Clipboard(String) 需 .to_string()）。

## 未解决问题

- Ctrl-C 经 \x03 不中断 ping -t/timeout/Test-Connection/Start-Sleep：**已由 2026-11-11 实验 X 解决**（根因：app 继承“忽略 Ctrl-C”标志；spawn 前 SetConsoleCtrlHandler(NULL, FALSE) 清除后均可中断；run13/15 14/14、Y 双法绿）；阶段1 E1-E12 数据全量见 vt-verify-win/logs/t4-probes.log 与 crates/pty/tests/pty_windows.rs 记录型探针。
- pwsh 7.6.6 + ConPTY FailFast（totalWidth=-17）：观察项，非阻塞（层级=pwsh 自身，见上）；windows.toml 示例 shell 在 pwsh 修复前于本机启动即崩。
- render 共享测试切片 bug 待 Linux 侧修复后 Windows 转绿。
- app 事件循环无运行级自动化测试（与 Linux 段同限制）；GUI 行为列 PHASE1-WIN.md 手动清单。
- ConPTY 主端 reader 在子进程退出后 EOF 延迟（验证期 W5 数据约 83s）；close 契约顺序已按此设计。

## 阶段 3S（2026-11-16）结论

- 合并 Linux 4T（`phase4t-done` = `8132ade`，轻量 tag）到 win，合并提交 `b926a01`，0 冲突；Windows 专属文件整体 sha256 合并前后一致（`8540f797…`）。状态=已验证事实。
- Windows 门禁（合并后）：debug/release 0 告警、clippy `-D warnings` dev/release 均 0、`cargo test --workspace` 211 passed / 0 failed、`wgpu 30.0.2`、`gpu_parity` + `LEANTTY_GPU_ALLOW_SOFTWARE=1`（DX12 WARP）10/10 通过。状态=已验证事实。
- `t6_probe_spawn_from_devtty_cwd` 硬编码 `C:\dev\tty`（仓库已迁到 E:）：未建等价目录时必失败；本轮以 `mklink /J C:\dev\tty E:\dev\tty` 复原环境后全绿。属测试硬编码，未改。状态=已验证事实。
- **Windows shell 退出检测（A=4r / B=4s / C=4t，各 12 组合 × 3 次）**：A 全部在子进程退出后 3.1–4.3 s 内退出；B 与 C 全部 15 s 内不退出（cpu 后端、系统 conhost 与随附 ConPTY 两种部署、cmd 与 pwsh 一致）。直接杀 shell 子进程：A 107–127 ms 退出，B/C 均不退出。状态=已验证事实。
- 因此 Linux 4T 对"4S 空闲静默引入退出检测回归"的判断在 Windows 成立；但 4T 的 1 秒存活检查在 Windows 上**未生效**（C 与 B 同表现），触发 3S 任务书停止条件（未改 crates/）。状态=已验证事实（表现）+ 工程判断（归因）。
- 定位证据（C）：子 shell 存活 30 s 无输出时 13 s 内仅 1 条 5 秒统计行（`loops=1`）→ 1 秒唤醒从未发生；杀 shell 后 4 s 不退出，随后一次窗口最小化/还原事件即立即退出 → `try_wait` 有效、缺的只是定时唤醒。状态=已验证事实。
- 待验证假设：存活唤醒块在空闲那次 `about_to_wait` 中被提前 `return` 跳过，或 `wait_scheduled` 残留为真；两者需在 crates 内加计数/日志区分（本任务不允许改）。推翻条件：加一行日志即可判定。状态=待验证假设。
- 上轮（3R）回报中的 keygen / SQLite / `send-keys --hex` / `tonight.sh` / `F7a`·`F7b` / `watchdog` 在仓库历史与工作区**均无命中**；`LEANTTY_VT` 不存在（仅 3R 自建脚本使用，app 不读取）；`process_group` 只出现在 ConPTY Ctrl 事件测试的进程组参数里。状态=已验证事实。
