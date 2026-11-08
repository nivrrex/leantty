# libghostty-vt Windows 原生构建与接口验证报告（W1~W8）

- 日期：2026-11-08（按系统时间）
- 目标：在 Windows 10 原生环境完成 libghostty-vt 构建与接口验证（Linux 段 V1~V8 的 Windows 复测，W5 为 ConPTY 专项），只验证不写正式项目代码
- 结论分级：已验证事实 = 本次环境实际运行得到；工程判断 = 基于事实的推断，无直接测量；待验证假设 = 尚未证实
- 复现方式：`powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\vt\checks\run_all.ps1`（顺序跑 W1~W8；W6 因记录到 Ctrl-C 未中断而按设计以非零码退出）

## 与 Linux 段的映射（范围说明）

| Windows | 对应 Linux 项 | 说明 |
|---|---|---|
| W1 | V1 | 构建产物 |
| W2 | V2 | 离线可重复构建（真实断网） |
| W3 | V3 | 下游工程链接 |
| W4 | V4 | 网格断言 a~f |
| W5 | V6 的 Windows 形态 | ConPTY 专项（cmd.exe + powershell.exe 两轮） |
| W6 | V7（+ 专项） | KeyEncoder 序列 + Ctrl-C 经 ConPTY |
| W7 | V8 + V9 探针 | 滚动语义记录（a~e）+ 快照用例 |
| W8 | V8 限制 #4 的补充 | release 基线 + 100MB 吞吐（Linux 段无 release/吞吐数据，仅并列可比项） |

## 环境（已验证事实）

| 项 | 值 | 来源 |
|---|---|---|
| 系统 | Windows 10 LTSC 19044.4529 | ver |
| Rust | 1.99.1 (58cf8f9c3 2026-10-13)，仅 x86_64-pc-windows-gnu target（无 msvc） | w1-rustc.txt |
| 链接器 | GNU ld 2.47（MinGW gcc 16.2.0，E:\dev\mingw64），经 collect2 | w1-linker.txt |
| Zig | 0.17.1 官方 Windows x86_64 zip，解压于 C:\vt\toolchain\zig，sha256 b03f477a1943673e02aca0f570e8030dffd536ce1f45cbc4d3326ec31159562e 与 index.json 一致 | 解压校验记录 |
| libghostty-rs | commit 84373f8d48b8f0b794e259f74a71a52686616f5d（v0.2.1，MSRV 1.96） | PINS.md |
| ghostty 源码 | commit 0b8b9984ee560456194262d190aa850224edb1f4 | PINS.md / vendor |
| MSVC/VS Build Tools | 无（vswhere 无输出）→ dumpbin 不可用，W3 用 MinGW objdump -p 取 DLL 依赖清单（工程判断：信息等价） | 勘察 |
| CARGO_TARGET_DIR | 全局环境变量指向 E:\dev\build\billy-target（所有 cargo 产物在该目录；W3 起下游工程脚本内另设本地 target） | env |
| LongPathsEnabled | 注册表实测 = 1（先决状态）；全程未遇 MAX_PATH 错误，无需"启用后重试" | w2c 记录 |
| 管理员权限 | 有（netsh/路由操作均成功执行） | 实测 |

Pins 与 Linux 段完全一致（Zig 0.17.1、上述两个 commit），未升级。vendor 源树复制自 Z: 源，抽样 sha256（build.zig、terminal.rs 等）逐字节一致；zig-system 24 个哈希目录复制，抽样 C 与 Z 逐字节一致，目录名无 CR/LF、最长路径 79 字符、前 50 个 zig/zon 源文件无 CR（复制未引入 CRLF 转换）。

构建机制事实（已验证事实）：build.rs 在 Windows 静态库名 ghostty-vt-static.lib、`static:+verbatim=` 链接；native 构建不传 -Dtarget；build.rs 中 /MD 与 /MT 出现 0 处。Zig 调用经 checks/zig-log.cmd 包装器记录全部实参；因本机无 Windows SDK，zig 原生默认 ABI（msvc）会报 LibCStdLibHeaderNotFound（w1-msvc-default-failure.txt），包装器在 `zig build` 且无 -Dtarget 时注入 `-Dtarget=x86_64-windows-gnu`（与 build.rs 自身目标映射表同值；工程判断：非绕过，是补齐本机缺失的 ABI 前提）。

## W1 原生构建：通过

- 已验证事实：`GHOSTTY_SOURCE_DIR=C:\vt\vendor\ghostty-src` + 包装器下 `cargo build --workspace` 成功（logs/w1-build.log）。rustc 1.99.1 host x86_64-pc-windows-gnu；链接器 GNU ld 2.47（collect2 16.2.0，E:\dev\mingw64）；zig 实参 `-Demit-lib-vt=true -Doptimize=Debug -Dcpu=baseline -Demit-xcframework=false -Dapp-runtime=none --prefix … --cache-dir … --system C:\vt\zig-system` + 注入 `-Dtarget=x86_64-windows-gnu`（logs/w1-zig-invocations.txt）。
- 已验证事实：产物 bin\ghostty-vt.dll 9,956,864 B、ghostty-vt.pdb 12,922,880 B、lib\ghostty-vt-static.lib 28,468,430 B、导入库、include\ghostty\vt\ 头文件 35 个（logs/w1-artifacts.txt、w1-headers.txt）；同时产 DLL（emit-lib-vt=true），非仅静态库。
- 失败原因记录（第一次尝试，不通过）：默认路径（不设 GHOSTTY_SOURCE_DIR）时 build.rs:486 `git clone ghostty failed with status exit code: 128`——github.com 本机不可达（logs/w1-build-default.log）。按约束不换库，改用 build.rs 自带 `GHOSTTY_SOURCE_DIR` 机制指向同一 pinned commit 的 vendored 副本后通过；两次结果并列记录如上。

## W2 离线可重复构建：通过（两次阻断尝试如实并列）

- 已验证事实（尝试 1，防火墙，无效）：本机三个防火墙配置文件初始全部 Disabled；启用配置文件后按程序路径规则（cargo/rustc/zig/git）+ 全出站阻断规则均未拦截任何出向（zig fetch 仍下载成功、cargo search 仍通、curl 仍 200；logs/w2-ps1-run.log）。附带发现：zig 的网络行为由其子进程 maker.exe 执行（路径在全局缓存 o\<hash>\ 下），按 zig.exe 路径的规则覆盖不到它（已验证事实）。
- 已验证事实（尝试 2，删默认路由，真实断网）：`route delete 0.0.0.0` 后三探针全部失败——zig fetch "server connection failed"、cargo search "failed to retrieve search results from the registry at https://crates.io"、curl "Could not resolve host" exit 6（logs/w2b-block-zig.log / w2b-block-cargo.log / w2b-block-curl.log）；随后 `cargo clean` + `GHOSTTY_SOURCE_DIR` + `GHOSTTY_ZIG_SYSTEM_DIR` + `cargo build --workspace --offline` 成功，6m41s（logs/w2b-offline-build.log）；zig 调用日志含 `--system C:\vt\zig-system` 且无 fetch（logs/w2b-zig-invocations.txt）。
- 已验证事实（哈希目录）：24 个子目录，目录名无 CR/LF，最长路径 79 字符，抽样哈希目录 C 与 Z 逐字节一致，前 50 个 zig/zon 源文件无 CR；LongPathsEnabled=1，全程无 MAX_PATH 错误（两种结果：未遇到，故无重试对比）。
- 恢复：路由已恢复（w2b-routes-after.txt 与 before 一致，curl 200）；防火墙配置文件恢复原始 Disabled（netsh，PowerShell Set-NetFirewallProfile 的 GpoBoolean 转换错误改用 netsh 完成）。
- 失败原因（尝试 1）：防火墙在本 VM 完全不起作用；环境原因未定位（工程判断：VM/VDI 网络旁路，待验证假设）。

## W3 下游链接：部分通过（默认链接行失败；下游加标志后全通过）

- 已验证事实（失败基线）：默认 rustc 链接行报 `relocation truncated to fit: IMAGE_REL_AMD64_REL32 against undefined symbol '___chkstk_ms'`（logs/w3-build.log/w3-y.log）。受控复现：用链接器包装器抓完整 exe 链接行 + 空 symbols.o 重放，逐字复现同一错误（logs/w3-linkline.txt、rp-B*.out）。
- 已验证事实（定位）：MinGW libgcc.a 的归档索引含 `___chkstk_ms in _chkstk_ms.o`（T，强定义）；rust 自身 libcompiler_builtins.rlib 亦含强定义 T；libstd 与 zig obj 均为 U；zig 产 obj 与最小复现 obj 的符号/重定位形态逐项一致（pe-x86-64，sec 0/scl 2，IMAGE_REL_AMD64_REL32）；最小 zig obj 独立经 gcc 链接成功；扫描顺序正常（项目 rlib 在前，compiler_builtins 与 -lgcc 在后）；唯一区分变量是 `-Wl,--gc-sections`：从链接行去掉即成功（B1/B3 重放），保留则失败（B6 对照），加 `-Wl,--no-gc-sections` 后 cargo 构建成功（其位置在 rustc 自带 --gc-sections 之后，后写覆盖）。
- 已验证事实（修复后）：下游工程 checks/v3_project（Cargo.toml path 依赖 ../../vendor/libghostty-rs/crates/libghostty-vt + portable-pty 0.8）构建成功，运行输出 `downstream link ok`（logs/w3-build2.log、w3-run.log）；objdump -p 依赖清单（logs/w3-dlls.txt）：KERNEL32、USERENV、WS2_32、api-ms-win-core-synch-l1-2-0、api-ms-win-crt-*（UCRT 前向层）、bcryptprimitives、ntdll；无任何 Zig 相关 DLL（grep zig = 0）；debug exe 19,270,911 B。
- 失败原因：GNU ld 的 --gc-sections 与 zig COFF 对象对 `___chkstk_ms` 的引用交互致归档抽取未解析（根因在 ld/zig 交互层，未进一步定位；工程判断）。修复全部在下游工程配置（.cargo/config.toml 加 `-Wl,--no-gc-sections` + 链接器包装器），未改上游代码、未换库。
- 已知附带事实：rustc 链接行含 `-nodefaultlibs`，CRT 经显式 -lmsvcrt/-lmingwex 等引入；dumpbin 缺位，依赖清单为 objdump 等价物（工程判断）。

## W4 网格断言：通过（6/6）

- 已验证事实：`cargo test --test v4_grid` 原样运行，v4a~v4f 全过，0.02s（logs/w4-grid.log）：a 纯文本首行 h/e/l/l/o；b SGR 红（默认 palette (204,102,102) 口径）；c `ESC[2J ESC[H` 后光标 (0,0) 且网格空；d 60 行 → scrollback_rows>=36 且视口末非空行 "line 60"；e "你好abc" 宽格断言（你/好各 2 格，SpacerTail 续格）；f `ESC[?1049h` → Alternate 且 scrollback_rows==0，`ESC[?1049l` 回 Main 且内容恢复。
- 失败原因：无。

## W5 ConPTY 专项：通过（2/2，附平台行为数据）

已验证事实（logs/w5-conpty-run2.log；启动字节原文 w5-cmd-raw.bin / w5-powershell-raw.bin）：

- cmd.exe 轮：启动 204 字节原文 hex 落盘；标记计数 ESC[6n=0、ESC[?9001h=0、ESC[?1004h=0、OSC(ESC])=2；on_pty_write/on_device_attributes 回调日志 16 字节 = 仅 "callbacks begin" 头，回写字节 0（与启动无 DSR 探针一致，回调未触发，不虚构）；默认 chcp 936 下 echo 的 UTF-8 字节经网格渲染为乱码（鐗堟湰等，控制台 GBK 呈现，日志为原文）；chcp 65001 后 "Active code page: 65001"，echo 你好 输出 `e4 bd a0 e5 a5 bd`，宽格断言 PASS（各 2 格）；resize 80x24→120x40：Terminal 侧 cols=120 rows=40，PTY 侧 mode 输出 Lines: 40 / Columns: 120，双侧一致断言 PASS；退出顺序 B（先 exit 后 drop master）：child.wait 9.06~9.12s 返回 code 0，master reader 15s 观察窗内无 EOF。
- powershell.exe 轮：启动 99 字节；OSC=1；`[Console]::OutputEncoding` 输出 BodyName gb2312 / CodePage 936（如实记录）；chcp 65001 与 echo 你好 宽格断言 PASS；resize 双侧一致断言 PASS；退出顺序 A（先 drop master）：child.wait 285.2µs 返回 code 3221225786（0xC000013A，STATUS_CONTROL_C_EXIT），reader 线程随即 EOF，join 立即返回。
- 关键平台行为（已验证事实）：同一次运行内，先 exit 时 ConPTY master reader EOF 延迟约 83s（旧二进制 join 实测 97.98s 总耗时中 83s 为 join 阻塞），先 drop master 时 EOF 立即；子进程退出本身两者均正常。
- 失败原因：无（测试断言全过）；实现注意：git-bash 的 timeout 无法终止 MinGW 子进程（忽略 SIGTERM），清理须用 taskkill/Stop-Process（harness 细节，已验证事实）。

## W6 KeyEncoder + Ctrl-C：部分通过

- 已验证事实（通过部分）：`cargo test --test v7_key` 2/2（v7_keys_normal_mode、v7_keys_application_cursor_mode），logs/w6-key.log；与 Linux V7 同口径。
- 已验证事实（不通过部分）：经 ConPTY 主端写入 `\x03` 字节未中断 cmd.exe 前台 `ping -t`：写 \x03 后 1.0s 静窗内仍收到新 ping 回复（TTL=128 行），无 ^C/Ctrl 标记，随后 `echo AFTER-CTRL-C` 未执行（外壳未回到交互态），断言 "shell should still be interactive after Ctrl-C" 失败（logs/w6-ctrlc.log、w6-ctrlc-run.log；原始字节流 w6-ctrlc-raw.bin）。
- 失败原因（待验证假设）：ConPTY 下经主端写字节流的 \x03 不等同 KEY_EVENT 方式的 Ctrl-C；本验证未实现事件级注入，未进一步定位。按约束不推断机制、只记录实测。

## W7 滚动语义与快照：通过

- 已验证事实（V9 a~e，只记录不断言，logs/w7-scroll.log）：60 行输出后 scrollback_rows=Ok(37)、视口 24 行；Top 后首行 "line 01"/末行 "line 24"；Bottom 后首行 "line 38"；Delta(5) 在底部无变化；Row(3) 后首行 "line 04"/末行 "line 27"；alt 屏 scrollback_rows=Ok(0) 且 Delta(3) 无效果；回主屏恢复 37。viewport_y/col/row 为行/图片级 API（非 Terminal 顶层滚动偏移），故按"scrollback_rows + 首末可见行文本"记录（工程判断）。
- 已验证事实（V8）：v8_snapshot 2/2（owned 快照不变性）；debug 构建 30 次取样中位数：80x24 = 991.2µs、200x60 = 6.4071ms（logs/w7-snapshot.log）。
- 失败原因：无。

## W8 release 基线：通过

- 已验证事实：vendor 工作区 `cargo build --release` 成功，4m52s（logs/w8-release-build.log）；zig 实参 `-Doptimize=ReleaseFast -Dcpu=baseline` + 注入 target（logs/w8-zig-invocations.txt）；产物 lib\ghostty-vt-static.lib 12,577,616 / 12,577,620 B（两次 release 构建，差 4 B，与 Linux 段同类产物字节非确定现象一致）、ghostty-vt.lib 56,892 B、bin\ghostty-vt.dll 2,471,424 B、ghostty-vt.pdb 8,179,712 B、头文件 32 个/目录（logs/w8-release-lib.txt）。
- 已验证事实（release 测试）：v8 快照中位数 80x24 = 168.4µs、200x60 = 1.0828ms（30 次取样，logs/w8-snapshot.log）；100MB 吞吐：feed=104,857,600 B（1600 × 65536B），elapsed=396.12ms，throughput=252.45 MiB/s，事后 scrollback_rows=Ok(620)（logs/w8-throughput.log）。
- 与 Linux 并列（仅可比项并列，不做换算）：

| 指标 | Linux debug（V8） | Windows debug（W7） | Windows release（W8） |
|---|---|---|---|
| 80x24 快照中位 | 1.22 ms | 991.2 µs | 168.4 µs |
| 200x60 快照中位 | 7.37 ms | 6.4071 ms | 1.0828 ms |
| 100MB 吞吐 | 未测（Linux 段无此项） | — | 252.45 MiB/s |
| 静态库大小 | libghostty-vt.a 85,849,368 B（debug） | ghostty-vt-static.lib 28,468,430 B（debug）/ 12,577,6xx B（release） | 同左 |

- 失败原因：无。Linux 段本身无 release 基线（其报告限制 #4），故 release/吞吐为 Windows 侧新增数据；跨机器数据仅并列记录。

## 未解决问题与已知限制

1. W3 根因：GNU ld `--gc-sections` 与 zig COFF 对象 `___chkstk_ms` 引用的交互未解析；下游 `--no-gc-sections` 为可行绕行（已验证），上游侧根因未定位（工程判断）。未改上游代码、未换库。
2. W6：Ctrl-C 经 ConPTY 主端字节流未中断 ping -t（已验证事实）；机制未定位（待验证假设：需 KEY_EVENT 级注入）。
3. W5：exit-first 路径 ConPTY master reader EOF 延迟约 83s（已验证事实）；其内部计时未定位。
4. W2：本机 Windows 防火墙对出向完全无效（配置文件 Disabled 与启用后均无效），真实阻断改用删默认路由实现；防火墙无效的环境原因未定位（工程判断：VM/VDI 旁路，待验证假设）。
5. 产物字节非确定：release static.lib 两次构建差 4 B；debug 与 Linux 段同类现象一致（其报告未解决问题 1）。
6. cargo 源替换：早前观察到 ustc 替换迹象，但配置文件未定位，`cargo config get` 仅 nightly 可用，本机 stable 无法复核（未核实）；W2b 的 crates.io 探针在真实断网下失败，与源替换无关。
7. MAX_PATH：全程未遇；LongPathsEnabled 实测先决为 1，无"启用后重试"对比可记。
8. 范围：Linux V5 的 11 项 API 清单对照在 Windows 映射中由 W4/W5/W7 的实际调用覆盖，未单独做清单级 grep 对照（任务定义如此）；V5 抽查口径限制同 Linux 段。
9. 控制台呈现：日志中中文乱码为 GBK 控制台呈现所致，原始日志文件（UTF-8/UTF-16）内容正确；Tee-Object 输出为 UTF-16，grep 需注意（工程判断）。

## 结论

W1~W8 中：W1/W2/W4/W5/W7/W8 通过，W3 部分通过（默认链接行失败 + 下游标志修复，全证据受控复现），W6 部分通过（KeyEncoder 2/2 通过；Ctrl-C 经字节流路径未中断 ping，按实测记录为不通过项）。全部结论均有 logs/ 下可重复脚本与原始输出支撑；未修改上游代码、未更换库。
