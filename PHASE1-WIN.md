# PHASE1-WIN：阶段1 Windows 交付

- 日期：2026-11-09（按系统时间）
- 范围：阶段1 的 Windows 部分，仅 CPU 渲染；未进入阶段2；未引入 GPU/wgpu/OpenGL 任何代码与依赖。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。
- AGENTS.md 冲突说明：仓库根 AGENTS.md 仅含 Context Gateway 压缩约定与 handoff-gate 节（其文本自述仅当 report_to_user 工具可用时适用，本机无该工具），与本提示词无实质冲突；按本提示词执行。

## 1. 完成项与测试结果

环境（已验证事实）：Windows 10 LTSC 19044.4529；Rust 1.99.1 仅 x86_64-pc-windows-gnu；Zig 0.17.1（C:\vt\toolchain\zig，pins 同 vt-verify/PINS.md，未升级）；portable-pty 0.8.1 ConPTY；构建经 ZIG/GHOSTTY_SOURCE_DIR/GHOSTTY_ZIG_SYSTEM_DIR 环境变量契约（scripts/windows/build.ps1，仓库内无绝对路径）。

| 项 | 结论 | 证据 |
|---|---|---|
| T1 构建脚本 + zig 包装器（去日志）+ rustflags | 通过 | scripts/windows/build.ps1、zig-inject-target.cmd；整工作区构建 OK（vt-verify-win/logs/t1-workspace-build.log）；.cargo/config.toml 仅 rustflags=-Wl,--no-gc-sections、无链接器包装器（实测仅 rustflags 足够；验证期受控重放 B1/B3/B6 定位 --gc-sections 交互，证据 C:\vt\logs\w3-*） |
| T2 windows.rs Pty 实现 | 通过 | crates/pty/src/windows.rs；close 先释放主端再 kill+wait；join_thread_timeout 5s 有界；空 shell 回落 cmd.exe；pty_windows 契约测试过 |
| T3 UTF-8 代码页 | 通过 | cmd /K chcp 65001、powershell -NoLogo -NoExit -Command chcp 65001；echo 你好 入网格且汉字占 2 格（cmd+powershell 双测试过）；默认 936 乱码为已验证事实记录 |
| T4 Ctrl-C | 未解决，按规格停止 | E1-E12 全量实测（vt-verify-win/logs/t4-probes.log；tests/pty_windows.rs 记录型探针）；\x03 仅中断读 stdin 的 sort，不中断 ping -t/timeout/Test-Connection；\x03\r/双写/间隔写/\x1c/PS 变体全败；FFI AttachConsole=0（ConPTY 拒外部附加）；不推断机制 |
| T5 on_pty_write 接线 | 通过 | pty_on_pty_write_dsr_response：喂 ESC[6n 断言回写字节（ESC[...R）到达 PTY 写端 |
| T6 app Windows 编译运行 | 通过 | cargo build --workspace OK；app.exe 起窗实测：SHELL=cmd.exe 与 SHELL=powershell.exe 双变体均 MainWindowTitle=tty、Responding=True、无 panic（vt-verify-win/logs/t6-app-*.log）；字体 Consolas（系统自带等宽族，fontdb 查找，无硬编码字体文件路径）；相对 shell 名经 WindowsPty::resolve_program 自解析（portable-pty search_path 在 bash 直启环境下失效，已验证事实） |
| T7 全量验证 | 部分通过 | cargo test --workspace：pty 21 过、vt 7 过、render 1 过 1 败、clippy rc=0、release rc=0（585s）；render 败因=共享测试切片假设 bug（见下），非 Windows 实现缺陷 |
| T8 交付 | 本文件 | bundle 交付见下 |

unix 门控：crates/pty/tests/pty_unix.rs 已加 #![cfg(unix)]（shared 提交 ef752e4）；全仓库无其他 /bin/sh 运行路径（crates/app/src/main.rs 的 SHELL 回退除外，见请求栏）。

## 2. 仓库状态

- git rev-parse win（PHASE1-WIN.md 提交前顶端）：0446484a9314be8f702ad351a0f943054d8277da
- 提交前 bundle（win-pre.bundle，不含本文件提交）SHA256：c52daaaaf70cdcb63f7ea8e864bf0acaa0bdee0134cc464a3eb81560a34d9fde
- git bundle verify（win-pre.bundle）输出：requires ref 94845e67c90b401b6b6c311ef78c8118b8c0ddd4（origin/main）；uses sha1；verify 通过。
- 自指约束如实说明：交付 bundle 在 PHASE1-WIN.md 提交之后生成才能包含该提交，其 SHA256/verify 无法写入本文件自身（自指）；交付 bundle 的 SHA256、verify 输出与最终 git rev-parse win 见交付消息及 docs/WIN-NOTES.md 交付记录。

## 3. 未解决问题

1. Ctrl-C（\x03 经主端写入）不中断非 stdin 型前台进程（ping -t/timeout/Test-Connection）；读 stdin 型（sort）可中断。机制未定位，按阶段规格停止，不推断。
2. 共享测试 render_pixels.rs 背景格断言按连续切片 fb[..16*16] 取左格，实际帧缓冲为 32 宽行主序，切片实为前 8 整行（含右格字形像素）；Linux 靠字体度量侥幸通过，Windows Consolas（X 自第 0 行起绘制）暴露。帧图证据：vt-verify-win/logs/probe-render6.log（左格零红像素、X 完整落于右格 16-23 列）。
3. ConPTY 主端 reader 在子进程退出后 EOF 延迟（验证期 W5 数据约 83s）；pty 实现已按 close 契约顺序设计。
4. app 事件循环无运行级自动化测试（与 Linux 段同限制）。

## 4. 对共享代码的请求（docs/WIN-NOTES.md 请求栏）

1. crates/app/src/main.rs：SHELL 未设置时回退 "/bin/sh" 在 Windows 无效，请按平台回落（Windows: cmd.exe）。
2. crates/render/tests/render_pixels.rs：背景格断言请按 (row*width+col) 索引取左格像素，替代连续切片。
3. （观察项，非阻塞）render blit 未裁 px>=cell_x 与 py>=cell_y；当前 Consolas 度量未触发越界。

## 5. 与规格的偏差

- T4 规格要求"找到可用办法并实现"；实测全部路径失败，按规格"全部失败则停止，附全部实测数据，不推断机制"执行，未实现变通（不变通即欺骗）。
- T7 因请求 2（共享测试 bug）未修复，cargo test --workspace 记 1 败；clippy 与 release 通过。
- main.rs SHELL 回退未擅改（请求 1），Windows 运行需 SHELL 环境变量。

## 6. 阶段1 限制（与 Linux 段一致）

- 键盘仅支持命名键 + ASCII 字母数字，其余忽略；IME 未实现；网格像素尺寸固定 8x16。
- 本清单不含输入法与可变字号项（阶段1 不支持）。

## 7. 用户本机手动确认清单（自动测试已覆盖项不列入）

1. `set SHELL=cmd.exe && cargo run -p app`：起 cmd.exe 窗口，键入 `echo 你好`，窗口显示中文且汉字占 2 格宽。
2. `set SHELL=powershell.exe && cargo run -p app`：同上起 powershell 窗口。
3. 窗口内 `ping -t 127.0.0.1` 后按 Ctrl+C：当前记录为不中断（与自动测试一致；未解决问题 1）。
4. 拖拽窗口边缘调整大小：字符网格同步重排、无残影（resize 双侧一致已自动覆盖，此为窗口级确认）。
5. 关闭窗口：进程立即退出、无卡死（close 契约运行级确认）。
6. 窗口内 F1/Home/End/Shift+Tab：shell 有响应（命名键映射自动测试已覆盖，此为运行级确认）。
7. 启动行显示 "Active code page: 65001" 属预期（spawn 参数置 UTF-8 的做法）。
8. 字体渲染为 Consolas（系统自带等宽字体族，无硬编码字体文件路径）。
