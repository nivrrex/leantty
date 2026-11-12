# PHASE2B-WIN：阶段2B Windows 复验交付

- 日期：2026-11-12（按系统时间）
- 范围：合并 Linux 阶段2B（tag phase2b-done = d278e76）后在 Windows 复验；建立 2B 自动化 e2e（剪贴板/选区/鼠标/滚动/IME/启动耗时/pwsh ConPTY 矩阵）；仅 CPU，未进入 2C，未做 GPU。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。
- AGENTS.md 冲突说明：仓库根 AGENTS.md 仅含 Context Gateway 压缩约定与 handoff-gate 节（无该工具时其自述不适用），与本提示词无实质冲突。

## 1. 合并与构建

基线 win 分支 63c8c7f（2A 收口 + clear_console_ctrl_ignore），`git fetch origin --tags` 后 `git merge phase2b-done`：

| 项 | 结论 | 证据 |
|---|---|---|
| 合并 | 无冲突，合并提交 fff7b4d | `git log -1 --stat`（带入 clipboard.rs/ime.rs/paste.rs/scrollbar.rs/selection.rs、PHASE2B-LINUX.md、background_load.rs/scroll_probe.rs 测试） |
| 既有 Windows 修复保留 | clear_console_ctrl_ignore 仍在 crates/pty/src/windows.rs:37-38 | grep |
| arboard 3.7.0 Windows 编译 | 失败→最小修复（shared: 63c8c7f） | E0432/E0433：GetExtLinux/SetExtLinux/LinuxClipboardKind 仅 unix；改 `#[cfg(unix)] use` + primary_get/primary_set 双实现（Windows 返 Err）+ E0308 `.to_string()` |
| debug 构建 | rc=0，19.7s（改动后增量）/13s（ccache 热） | vt-verify-win/logs/2b-build.log |
| release 构建 | rc=0，10m54s（全量，含 libghostty-vt-sys） | 同上；产物在 CARGO_TARGET_DIR（本机全局 E:\dev\build\billy-target） |
| cargo test --workspace | 21s，仅 config `path_priority_cli_env_default` 已知平台性失败 | 其余全绿 |
| cargo clippy | rc=0，16s | 同上 |

构建入口（参数化，仓库内无绝对路径）：

```
powershell -File scripts/windows/build.ps1 -Zig <zig.exe> \
  -GhosttySourceDir <ghostty-src> -ZigSystemDir <zig-system> [-ExtraCargoArgs --release]
```

## 2. 自动化 e2e（新增）

| 脚本 | 覆盖 | 结果 |
|---|---|---|
| scripts/windows/e2e/wb2_lib.ps1 | 共享 Win32 助手（EnumWindows 按 pid+class 取真窗、隐藏 app 自带控制台窗、前台解锁、keybd_event 带 scancode/扩展键、mouse_event 绝对坐标、dump 读取 UTF8、行查找重试） | 被下列脚本 dot-source |
| run_2b_clip.ps1 | 剪贴板与选区：n1 拖选复制汉字、n2 粘贴汉字、n3 粘贴多行换行、n4a/n4b 1MB 粘贴、n5 括号粘贴、n5b 负控、n6 PasteSelection、m2 双击选词、m3 三击选行、m4 Shift+点击扩展 | 11 PASS / 0 FAIL（run11） |
| run_2b_mouse.ps1 | k1 Ctrl+Insert 复制、k2 Shift+Insert 粘贴、w1 滚轮上滚暂停 auto-follow、w2 滚动条拖拽、w3 ScrollToTop/Bottom 绑定、s1 SGR 上报 | 6 PASS / 0 FAIL（run10） |
| run_2b_perf.ps1 | release 启动耗时 5 次×2 配置、启动即键入不丢字符 | 3 PASS / 0 FAIL（run3） |
| run_mouse_diag.ps1 | 手势隔离诊断（拖选/双击/三击/Shift 点击对照） | 诊断用 |

运行方式（STA 必须：剪贴板 API）：

```
powershell -STA -NoProfile -ExecutionPolicy Bypass -File scripts/windows/e2e/run_2b_clip.ps1
```

## 3. 已验证事实（一行一条）

1. 拖选复制汉字可用：dump 行 `你好` 拖选 + 中键 Copy → 剪贴板 `你好`（n1 PASS）。
2. 右键粘贴汉字可用：剪贴板 `PASTECJK你好` → 屏幕上出现该串（n2 PASS）。
3. 粘贴多行：`echo NLINE1\necho NLINE2` 两行均执行（LF→CR 规范化生效；n3 PASS）。
4. 1MB 粘贴**静止时只投递 131066 字节**（约 2×64KB 块）后停住；同一操作下持续产生事件（鼠标移动）则 1048576 字节全部送达（n4a/n4b）。根因见 §5 请求 1。
5. 括号粘贴：探针进程自行发 `ESC[?2004h` 后，其收到的粘贴字节仅 `BRKTXT`（6 字节），无 `ESC[200~`/`ESC[201~`；cmd 负控同（无包裹）。ConPTY 输入通道自行吞掉标记（§4 事实 1）。
6. PasteSelection（PRIMARY）在 Windows 为 no-op：F12 绑定 PasteSelection 后剪贴板内容未进入 PTY，app 存活（n6 PASS；arboard Windows 无 PRIMARY，Get 返 Err 并打印 `copy primary: clipboard error`）。
7. 双击选词 / 三击选行 / Shift+点击扩展均可用（m2/m3/m4 PASS；双击得 `beta`、三击得整行）。
8. 双击判定阈值 300ms（crates/app/src/lib.rs start_selection）：e2e 点击间隔必须显著小于 300ms，否则退化为单格（首次全红即此因）。
9. Ctrl+Insert 复制、Shift+Insert 粘贴可用（k1/k2 PASS；k1 在拖选后清空剪贴板再按 Ctrl+Insert，排除"拖选释放自动复制"的假阳性）。
10. 滚轮上滚 5 格（multiplier=10）→ 视口从 LN300 退到 252，且新输出不再把视口拉回（w1 PASS）。
11. 右缘 4px 滚动条拖拽可用：拖到轨道上方后视口由 300 → 29（w2 PASS）。
12. ScrollToTop/ScrollToBottom 为可绑定动作（非默认键）：Ctrl+Home → 视口到 27，Ctrl+End → 回 300（w3 PASS）；滚动动作立即重绘，无需额外事件（诊断 redrawdiag：poked 前后同为 27）。
13. SGR 鼠标上报在 Windows 子进程侧不可观测：即使子进程发 `?1000h/?1002h/?1003h + ?1006h`，点击后子进程收到 0 字节（s1 PASS 断言的即此平台事实）。
14. release 启动首帧（[perf] first_frame_ms，5 次）：默认配置 278/333/282/389/319 ms，中位 319 ms、极差 111 ms；ps-win.toml（Microsoft YaHei Mono + 回退）2038/2266/2071/2145/2018 ms，中位 2071 ms、极差 248 ms。
15. 启动即键入不丢字符：窗口出现后立刻键入 `echo earlyinput` 并回车，回显与执行均可见（t4-early-input PASS，debug 二进制；release 无 dump 钩子）。
16. IME：app 启动即 `window.set_ime_allowed(true)`（crates/app/src/lib.rs:370）；实测启动不 panic、窗口/首帧正常（IME_SMOKE alive=True panic=False）；ime 单测 3 项通过（normalize_ime_commit_newlines、preedit_display_empty_and_clamp、preedit_display_none_cursor），app lib 共 19 项通过。
17. pwsh 7.6.6 经 ConPTY 启动必 FailFast：flags 0/1/2/4/6/7 × 尺寸 80x24/120x30/200x50 共 18 组合全部 `exit 0xC000013A`，输出含 `totalWidth`/`Process terminated`/`FailFast`；创建后 resize（160x40）同样崩。
18. `PSEUDOCONSOLE_INHERIT_CURSOR(0x1)` 不修复崩溃，只改变时序：带 0x1 时 pwsh 先发 `ESC[6n` 并存活等待，应答 `ESC[1;1R` 后仍崩（exit 0xC000013A）。
19. 同一 harness 下 powershell.exe 5.1 不崩（窗口内 400 字节输出、无 totalWidth、存活至窗口结束）；pwsh 7.6.6 不经 ConPTY（本 shell 直接执行）正常输出。
20. arboard Windows 剪贴板路径可用：Set/Get 与 app 的 Copy/Paste 双向一致（n1/n2/n3）。
21. ConPTY 输入通道吞掉 `ESC[200~AB ESC[201~` 中的标记，只把 `AB` 交给子进程（probe_ptyin，独立于 app）。
22. ConPTY 输入通道吞掉 SGR 鼠标序列：写入 `ESC[<0;10;5M` 后子进程收到 0 字节（probe_ptyin）。
23. e2e 注入细节（已验证）：`keybd_event` 必须带 scancode（`MapVirtualKey`），且 Insert/Delete/Home/End/PgUp/PgDn/方向键必须置 `KEYEVENTF_EXTENDEDKEY`，否则被当作小键盘键、winit 拿不到 logical key，绑定不触发。
24. 合成大写字母会让 `SHIFT` 在 winit 侧保持按下状态，之后所有键盘绑定失配（同进程内）；e2e 文本因此仅用小写。
25. 窗口刚激活后的第一次左键点击可能被吞（双击退化为单格）；e2e 先点一次空位"预热"。
26. 应用自身控制台窗口（ConsoleWindowClass）必须隐藏，否则抢焦点，点击/按键落到错误窗口（首轮全红的成因）。

## 4. 工程判断（一行一条）

1. 括号粘贴与 SGR 上报在 Windows 上不可端到端验证：ConPTY 在输入通道就把这两类标记消费掉（事实 21/22 为独立于 app 的最小复现），因此 app 侧是否包裹/是否上报只能由 Linux e2e（case13/case16）保证；Windows 侧记录为平台限制。
2. pwsh 7.6.6 的 FailFast 属 pwsh 自身对 console 尺寸/光标协商的假设失败（崩溃点 totalWidth，发生在 INHERIT_CURSOR 的 ESC[6n 应答之后），不是 app 渲染或 ConPTY flags 选择问题：18 组合全崩且 5.1 不崩（事实 17-19）。
3. 1MB 粘贴停摆的位置在 app 的排空策略而非 PTY 写入：补齐事件后全部送达，说明数据本身可写通（事实 4）。
4. 剪贴板/选区/绑定/滚动在 Windows 与 Linux 行为一致（除 PRIMARY 与上述平台限制），说明 2B 的共享实现未引入平台分支缺陷。

## 5. 对共享代码的请求（docs/WIN-NOTES.md 请求栏）

1. **1MB 级粘贴排空依赖事件循环唤醒**（P1，影响功能完整性）：`about_to_wait` 每轮只写一个 pending 块，写完不自触发下一轮；无新事件时排空停摆（实测 131066/1048576）。建议在 push 块后 `request_redraw()`，或单轮按预算循环排空。属共享行为变更，本阶段未改。
2. 无其他请求：arboard 的 cfg 拆分已按最小改动在 63c8c7f 完成。

## 6. 与规格的偏差

1. 任务 2 的"括号粘贴断言 ESC[200~/ESC[201~"改为记录平台事实（ConPTY 剥离），未能断言包裹本身（证据：事实 21）。
2. 任务 3 的"SGR 鼠标上报"同上（事实 22），断言改为"子进程收到 0 字节"。
3. 任务 3 的"End 回底"改为绑定 ScrollToBottom（Ctrl+End 与 plain End），因 End 单独按键默认是发往 PTY 的普通键。
4. 任务 6 的矩阵通过**临时**修改本机 registry 中 portable-pty 0.8.1 的 flags 来源（读 `APP_CONPTY_FLAGS`，默认值不变）完成，跑完已还原（`grep APP_CONPTY_FLAGS` = 0），未纳入仓库、未改仓库依赖。
5. 任务 4 的启动耗时以 release 二进制的 stderr `[perf] first_frame_ms` 为准；release 无 dump 钩子，故"输入不丢"一项用 debug 二进制验证。

## 7. 用户本机手动确认清单（≤3 条）

1. 真实输入法（微软拼音）在 app 内输入中文：候选窗位置、上屏、退格取消 preedit（本机无自动化输入法注入手段）。
2. 真实 Windows Terminal / 另一终端粘贴 1MB 文本做对照，确认"事件驱动排空"差异是否为 Windows 通例（本机仅 ConPTY 路径可测）。
3. pwsh 7 在 Windows Terminal 中启动是否正常（用于确认崩溃仅限 portable-pty ConPTY 组合）。

## 8. 交付物与证据

- 代码/脚本：`scripts/windows/e2e/{wb2_lib.ps1,run_2b_clip.ps1,run_2b_mouse.ps1,run_2b_perf.ps1,run_mouse_diag.ps1,cfg/*.toml}`；探针 `vt-verify-win/{probe_paste,probe_ptyin,probe_conpty,probe_pwsh}`。
- 测试配置：`cfg/clip-win.toml`（cmd）、`cfg/ps-win.toml`（powershell.exe）、`cfg/pastesel.toml`（F12=PasteSelection）、`cfg/scroll-win.toml`（End/Ctrl+End→ScrollToBottom）。
- 日志：`vt-verify-win/logs/2b-clip-run{1..11}.log`、`2b-mouse-run{1..10}.log`、`2b-perf-run{1..3}.log`、`2b-build.log`；用例 dump/err 在 `vt-verify-win/logs/phase2b-win/`。
- 探针矩阵原始输出：`vt-verify-win/logs/probe_pwsh_matrix.log`（本次补充见本节命令）。

## 9. 推送核对（push 后补记）

（见文末提交记录）
