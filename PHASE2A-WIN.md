# PHASE2A-WIN：阶段2A Windows 复验交付

- 日期：2026-11-11（按系统时间）
- 范围：合并 Linux 阶段1收口与阶段2A 成果（基线 631587e，含阶段1 win 分支全部工作）后在 Windows 复验；建立 Windows 自动化 e2e（scripts/windows/e2e/run_all.ps1，真实键盘路径）并重新验证 Ctrl-C；仅 CPU、无 GPU/wgpu/OpenGL；未进入阶段2B。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。
- AGENTS.md 冲突说明：仓库根 AGENTS.md 仅含 Context Gateway 压缩约定与 handoff-gate 节（其文本自述仅当 report_to_user 工具可用时适用，本机无该工具），与本提示词无实质冲突；按本提示词执行。

## 1. 完成项与测试结果

环境（已验证事实）：Windows 10 LTSC 19044.4529；Rust 1.99.1 仅 x86_64-pc-windows-gnu；Zig 0.17.1（C:\vt\toolchain\zig，pins 同 vt-verify/PINS.md，未升级）；portable-pty 0.8.1 ConPTY；构建经 ZIG/GHOSTTY_SOURCE_DIR/GHOSTTY_ZIG_SYSTEM_DIR/CARGO_TARGET_DIR 环境变量契约（脚本参数化，仓库内无绝对路径）。

| 项 | 结论 | 证据 |
|---|---|---|
| T1 合并后构建/测试/clippy | 通过（1 项共享测试平台性失败入请求栏） | vt-verify-win/logs/t1-merged.log：debug 58s、release 42s、clippy rc=0；cargo test --workspace 仅 config path_priority_cli_env_default 败（断言含 Unix /tmp 路径假设，Windows 平台性失败）；pty 21/vt 7/render/app 其余全绿（render 切片断言转绿即请求2/3 修复生效） |
| T2 Windows 自动化 e2e | 10 PASS / 3 FAIL（3 项为已验证限制，见下） | scripts/windows/e2e/run_all.ps1；vt-verify-win/logs/e2e-run11.log：release-hook-gated、a/b/c/d/e/f/g/h、l 全 PASS；i/j/k FAIL（ping -t/Start-Sleep 不中断，TTL 续增） |
| T3 Ctrl-C 真实键盘路径复验 | 链路通；非 stdin 型进程不中断（与阶段1 E-series 一致） | probe17（vt-verify-win/run_keyboard_probe17.ps1 + logs/e2e-probe/probe17.trace）：keybd_event Ctrl+C 四事件到达 app（KEYDBG），sort 被中断（dump 现新提示行）；e2e case l 同结论；ping/sleep 不中断系 \x03 作原始 stdin 字节、不读 stdin 者不受理（阶段1 E1-E12 全量数据 t4-probes.log） |
| T4 阶段2A 项 | 4/4 PASS | scripts/windows/e2e/run_2a.ps1；vt-verify-win/logs/2a-run2.log；字体家族实测 vt-verify-win/probe_font（fontdb 0.21 同库同查询链） |

e2e 用例：0 release 钩子门控（release 二进制无 APP_DUMP_SNAPSHOT 字符串）；a cmd echo、b pwsh5 echo、c CJK 宽格、d 退格/方向键、e WM_CLOSE 5s 退出、f exit 命令退出、g 缩放双侧一致（80→198 cols / 24→56 rows，mode con 匹配）、h 默认 shell cmd.exe、i/j/k Ctrl-C（cmd ping -t / PS ping -t / PS Start-Sleep）、l Ctrl-C 正控（sort 中断）。

## 2. 已验证事实（一行一条）

1. SendInput 在本机不投递：返回 2（接受）但事件零到达（probe15 交替实测：同运行内 keybd_event 的 a/c 到达且 cmd 回显 ac，SendInput 的 VK 与 scancode 变体均零到达）；e2e 注入层用 keybd_event（同一内核输入流，真实键盘路径）。
2. keybd_event 路径端到端真实：事件→winit KeyboardInput（KEYDBG 插桩，debug-only，已撤）→route_key→encoder→PTY→cmd 回显/dump 可见。
3. app 为 console 子系统：启动自带可见 ConsoleWindowClass 控制台窗（会抢焦点）；e2e 启动后隐藏之。
4. Get-Process MainWindowHandle 命中 6x6 隐藏 "Winit Thread Event Target" 工具窗；真窗须按 class "Window Class"（标题 tty）EnumWindows 枚举；同桌面另有 PI agent 同名 "tty" 窗，pid 为唯一权威。
5. ConPTY spawn 后 app 的 fd 2 失效（其后 eprintln 全部丢失，config 警告在 spawn 前故可见）；诊断改用 APP_TRACE_FILE 文件直写（插桩已撤）。
6. fontdb 本机枚举：Microsoft YaHei Mono / Microsoft YaHei 均存在且含汉字（has_han=true）；早先"本机无 YaHei"记录系枚举方法错误，作废；泛型 Monospace→Courier New、SansSerif→Arial（均无汉字）。
7. pwsh 7.6.6 在本机 ConPTY 下启动即 FailFast（totalWidth=-17），80x24 与 120x30 同现；powershell.exe 5.1 正常（e2e b PASS）；windows.toml 的 shell=pwsh.exe 启动即崩，系 pwsh+ConPTY 交互，非 app 字体/渲染缺陷（banner 已经网格渲染）。
8. 首帧耗时（Start-Process→真窗+首内容）：默认配置 4.2s / windows.toml 16.2s（YaHei Mono from_bytes 一次性成本，同 Linux 6.1s 量级）/ Consolas+YaHei 5.8s。
9. CJK 逐字回退：Consolas 主字体无汉字 → 组内回退面 Microsoft YaHei 承接 CJK 两格（dump font_indices 该行 2/100 非零，其余 98 格 0）。
10. 前台解锁配方：SPI_SETFOREGROUNDLOCKTIMEOUT=0 + AttachThreadInput(app UI 线程) + SetFocus + SetForegroundWindow + GetForegroundWindow 非零校验（probe13/15 实证）。
11. WM_CLOSE 经 PostMessage 发至真窗 → app 5s 内退出且无 wake 错误（case e PASS）；缩放 SetWindowPos → 网格行列双侧重算一致（case g PASS）。
12. windows.toml 加载：dims 120x30 生效、无字体加载错误、banner 经配置字体链渲染（case a PASS）。
13. 错误配置（未知字段/非法颜色/越界数值）：stderr 三类带路径警告、字段级回落默认、app 仍启动并渲染（case b PASS）。
14. config 共享测试 path_priority_cli_env_default 在 Windows 必败（断言期望 /tmp 前缀路径，系 Unix XDG 假设）。
15. origin/main 在本任务基线后推进至 2B（eba3579 lazy-load 字体、8ac57fc 选区/剪贴板/绑定，tags phase2b-t1/t2）；win 未合并该增量（任务范围止于 2A）。

## 3. 工程判断（一行一条）

1. SendInput 失效归因本机环境（同运行内 keybd_event 对照到达），非 e2e 脚本结构问题；keybd_event 与 SendInput 同属 Windows 输入注入 API族，e2e 的"真实键盘路径"目标未变。
2. i/j/k 保留 FAIL 而非跳过：规格要求对 ping -t 等复验；失败即结论数据（TTL 续增 1→14、marker=False）。
3. SetConsoleCtrlHandler(NULL,FALSE) 自身调用变体判为 moot：app 未注册任何控制台 handler，其控制台窗又非键盘输入目标，该调用不处于 Ctrl-C 投递路径上；无实现价值，不改共享代码（按规格仅"实测有效才入 windows.rs"）。
4. case c 由"CJK 键入"改为"子进程输出 CJK"（ASCII 键入 powershell [char] 命令）：keybd_event 无法注入 Unicode、本机 SendInput 失效；断言对象（CJK 宽格渲染）不变。
5. pwsh 崩溃记为观察项而非阻塞：windows.toml 字体项本身全部通过；pwsh 行为属上游 pwsh/ConPTY 交互。

## 4. 未解决问题（与阶段1衔接）

1. Ctrl-C 经 \x03 不中断非 stdin 型前台进程（ping -t/timeout/Test-Connection/Start-Sleep）；真实键盘路径已证链路通（sort 正控），子进程语义未变；机制未定位，按规格不推断。
2. 共享测试 render_pixels.rs 背景格切片 bug 待 Linux 侧修（请求 2；阶段1 登记，本轮 T1 render 转绿系该请求 2/3 已由 Linux 侧修复生效）。
3. ConPTY 主端 reader EOF 延迟（约 83s）依旧；close 契约顺序已按此设计。
4. config path_priority_cli_env_default 共享测试 Unix 路径假设（请求 4）。

## 5. 对共享代码的请求（docs/WIN-NOTES.md 请求栏）

1. （阶段1 请求 1）main.rs SHELL 未设置时 Windows 回退 cmd.exe（Windows 侧以 SHELL 环境变量绕过）。
2. （阶段1 请求 2）render_pixels.rs 背景格断言按 (row*width+col) 取左格像素——已由 Linux 侧修复，本轮 render 转绿。
3. （阶段1 请求 3，观察项）blit 未裁 px>=cell_x/py>=cell_y。
4. （新增）config 测试 path_priority_cli_env_default 断言依赖 /tmp 前缀，请在共享测试中按平台构造期望路径。
5. （新增观察项，非阻塞）pwsh 7.6.6 + ConPTY FailFast totalWidth=-17；windows.toml 示例 shell 在 pwsh 修复前于本机启动即崩。

## 6. 与规格的偏差

- e2e 键盘注入用 keybd_event 替代 SendInput（本机 SendInput 不投递，已验证事实 1；偏差原因与对照证据同第 2 节）。
- case c 输入方式改为子进程输出 CJK（工程判断 4），断言不变。
- SetConsoleCtrlHandler(NULL,FALSE) 变体未实现（工程判断 3：moot）。
- HiDPI 项为环境限制记录（VM 100% 缩放），真实 HiDPI 确认列入手动清单。

## 7. 用户本机手动确认清单（≤3 条，延后至 2B 后）

1. 真实 HiDPI 显示器下 ScaleFactorChanged 重排网格无闪烁、字形不模糊（VM 100% 缩放无法触发；cell_metrics 缩放性已单测覆盖）。
2. pwsh 7.6.6 更新版本或真实 ConPTY 主机下 windows.toml 启动是否仍 FailFast。
3. 真实显示器下 Consolas+Microsoft YaHei 回退链中西文混排观感（归属已由 font_indices 自动断言）。

## 8. 交付物与证据

- scripts/windows/e2e/run_all.ps1（e2e 入口）、scripts/windows/e2e/run_2a.ps1（2A 项）、vt-verify-win/probe_font（fontdb 家族探针）、vt-verify-win/run_keyboard_probe*.ps1（定位链探针）、vt-verify-win/logs/（t1-merged.log、e2e-run9/10/11.log、2a-run2.log、e2e-probe/、phase2a-win/、e2e-run1-8 定位过程日志）。
- 复验基线：win = 631587e + 本轮提交；未合并 origin/main 的 2B 增量（已验证事实 15）。
- 提交与推送核对：见 git log 与下方补记（push 后 ls-remote 核对 win hash 记入此处）。

## 9. 推送核对（push 后补记）

- 提交：35f2901fe94e519dd26f01a48ca66008155177a5（verify-win: 阶段2A Windows 复验 + 自动化 e2e + Ctrl-C 重验）
- git push origin win：24c1333..35f2901 win -> win（成功）
- git ls-remote origin refs/heads/win：35f2901fe94e519dd26f01a48ca66008155177a5，与本地 git rev-parse win 一致（2026-11-11 本机核对）
