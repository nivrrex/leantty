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
- ConPTY spawn 后 app 的 fd 2 失效（其后 eprintln 全部丢失）；诊断改用 APP_TRACE_FILE 文件直写。
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
- 请求 12（3C，待验证）：大段输出在 app 内推进极慢——debug 二进制下 app 内 cmd 执行 `type` 20 000 行（1.28 MB）180 s 内未到结束标记，dump 停在 L000050；100 行 60 s 内未完成；同时段 APP_PERF_FRAMES 显示 render_ms 中位 1–3 ms（release）/16–18 ms（debug）、present 1–2 ms、coalesced≈130，渲染侧空闲。建议在读线程→channel→vt.feed 路径统计每秒喂入字节数，确认是否存在节流。未定位，仅现象记录。
- 事实变更（3C，已验证）：上一轮把 `ctrl_c_probe_powershell_ping` 的 0xC000013A 归给 clear_console_ctrl_ignore **是错的**。A/B 对照（启用/禁用测试内 reignore）均 5/5 通过；真因是 `t4_e8_ffi_ctrl_c_to_ping` 在 AttachConsole 失败后调 `GenerateConsoleCtrlEvent(0, 0)`，进程组 0 广播到调用者自身控制台组，把 harness 杀掉。已在该测试内修（仅测试进程重新忽略 Ctrl-C），windows.rs 未改。
- 请求 13（3D，P1 数据丢失，已验证事实）：`crates/app/src/lib.rs:1233` 的 `WindowEvent::Resized` 未过滤最小化/零尺寸——最小化时窗口收到 1×1，行列被 `min(1)` 钳位后 `vt.resize(1,1)` 把终端内容裁掉；Windows 实测最小化期间 dump 头为 `# cols=1 rows=1`，恢复后文本丢失（暗像素 4465→2104，逐像素 diff 5413，基线噪声 0；`APP_FORCE_FULL_REDRAW=1` 同样不救）。建议 cols/rows 为 0（或 physical_size 任一维为 0）时跳过 vt/pty resize，或记住最小化前尺寸。未改共用代码。
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
