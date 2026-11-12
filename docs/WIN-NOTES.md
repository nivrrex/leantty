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
