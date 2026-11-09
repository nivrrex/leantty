# PHASE1-MERGE：Windows 阶段1 成果合并与 Linux 验收

- 日期：2026-11-09（按系统时间）
- 范围：合并 origin/win 阶段1 成果、处理 Windows 侧 3 条共享代码请求、Linux 重跑验收、打 tag phase1-merged。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实的推断；待验证假设 = 未证实。
- AGENTS.md 冲突说明：仓库根 AGENTS.md 仅含 Context Gateway 压缩约定与 handoff-gate 节，与本任务提示词无实质冲突；按本提示词执行。

## 1. 合并前审查（已验证事实）

- `git merge-base main origin/win` = 94845e67c90b401b6b6c311ef78c8118b8c0ddd4 = main 当前顶端（预期相等，符合）。
- `git diff --stat main..origin/win`：168 files changed, 7472 insertions(+), 2 deletions(-)。
- shared 改动仅预期两处：crates/pty/src/lib.rs（`#[cfg(windows)] pub use windows::WindowsPty as NativePty;` + 模块注释）与 crates/pty/tests/pty_unix.rs（`#![cfg(unix)]` 门控）；.cargo/config.toml 增 windows-gnu 段 rustflags（Windows 专属配置，规格允许）。其余改动均在 Windows 专属文件（windows.rs、pty_windows.rs、scripts/windows/、docs/WIN-NOTES.md、PHASE1-WIN.md、vt-verify-win/）。无清单外 shared 文件改动，审查通过。

## 2. 合并与三条请求（已验证事实）

- 合并提交：9369c72 `merge: win phase1 (Windows pty/build/scripts)`（--no-ff，无冲突）。
- 合并后 Linux 验收：`cargo build --workspace` 通过（3.59s）；`cargo test --workspace` 编译失败——pty_windows.rs 缺 `#![cfg(windows)]` 门控（引用 pty::windows::join_thread_timeout / WindowsPty::resolve_program / NativePty::child_pid，Linux 不存在，E0432/E0433/E0599 共 4 处）。按用户裁决 C 豁免处理（见"豁免记录"）。
- 请求 1 提交 8291c81：`app::default_shell()` 纯函数按 `cfg!(windows)` 回落 cmd.exe，其他平台 /bin/sh；main.rs 经 `unwrap_or_else` 调用（SHELL 未设置或非 UTF-8 均回落）；单测 default_shell_matches_platform 1 passed。
- 请求 2 提交 53339c3：render_pixels.rs 背景格断言从连续切片 `fb[..16*16]` 改为按 `(row*width+col)` 逐像素索引取左格区域；断言设计：空白格内全部像素=背景色、有字符格内至少一像素≠背景色；不依赖具体字体度量（原 Linux 侥幸通过、Windows Consolas 暴露的问题根因是行主序宽帧缓冲上连续切片实为前 8 整行）。render 2 passed。
- 请求 3 提交 9cb99a8：blit 裁剪条件改为单元格矩形四界 `[cell_x, cell_x+cell_w) x [cell_y, cell_y+cell_h)`（原实现缺 px>=cell_x 与 py>=cell_y 左上界，负 xmin/ymin 字形可侵入邻格）；合成字形单元测试 2 项（负偏移+超宽 → 右邻格不变；超高 → 下邻格不变）2 passed。
- clippy 修复提交 be5460d：field_reassign_with_default（metrics 构造）+ manual_contains×2。

## 3. 验收（Linux，全部通过；已验证事实）

- `cargo test --workspace`：13 passed / 0 failed（app 1、pty 1、render 4、vt 7），耗时 0.72s（增量）。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过（EXIT 0），耗时 1.54s（增量）。
- `cargo build --release`：通过（EXIT 0），耗时 2.61s（增量）；产物 target/release/app（8,600,192 字节，ELF x86-64）。

## 4. 未解决问题（保持原样引用 Windows 侧记录）

1. Ctrl-C（\x03 经主端写入）不中断非 stdin 型前台进程（ping -t/timeout/Test-Connection）；读 stdin 型（sort）可中断。机制未定位，Windows 侧按规格停止，不推断（PHASE1-WIN.md 未解决问题 1；vt-verify-win/logs/t4-probes.log）。
2. ConPTY 主端 reader 在子进程退出后 EOF 延迟（验证期 W5 数据约 83s）；pty 实现已按 close 契约顺序设计（PHASE1-WIN.md 未解决问题 3）。
3. app 事件循环无运行级自动化测试（容器无显示器；Linux 与 Windows 同限制）。

## 5. 给 Windows agent 的复验交接（仅已验证内容）

- 执行：`git fetch origin && git merge origin/main && cargo test --workspace`（Windows 本机，命令同 README 环境变量契约）；预期此前失败的 render 共享测试转绿（请求 2 已修切片断言为逐像素索引，请求 3 已补 blit 四界裁剪）。
- 已修确认：请求 1 main.rs SHELL 回退按平台（Windows 分支回落 cmd.exe，代码在 crates/app/src/lib.rs default_shell()；Windows 侧运行验证未做，属本机手动确认项）；请求 2 背景格逐像素断言；请求 3 blit 四界裁剪。
- 注意事项：pty_windows.rs 头部已有 `#![cfg(windows)]`（豁免提交 b092fc7），merge origin/main 时若该文件有改动请保留门控行；豁免仅此一处，不构成先例。
- clippy 验收口径：`cargo clippy --workspace --all-targets -- -D warnings`（阶段1 旧口径为 --workspace，all-targets 覆盖 tests）。

## 6. 用户本机手动确认清单（≤8 条；自动测试已覆盖项不列入）

1. `cargo run --release -p app` 起 shell（SHELL 未设置时应回落 /bin/sh），键入 `echo 你好` → 窗口显示中文且汉字占 2 格宽。
2. 窗口内运行 vim → 全屏交替屏渲染，退出 vim 恢复主屏内容。
3. 拖拽窗口边缘调整大小 → 字符网格同步重排、无残影（resize 双侧一致已自动覆盖，此为窗口级确认）。
4. 关闭窗口 → 进程立即退出、无卡死（close 契约运行级确认）。
5. vim 内方向键（应用光标键模式）与 shell 内（普通模式）移动行为均正确。
6. Ctrl-C 终止前台进程、Ctrl-D 退出 shell。
7. F1/Home/End/Shift+Tab 按键 → shell/vim 响应与预期一致（V7 字节序列已自动断言，此为运行级确认）。
8. 起 cmd.exe/powershell.exe 变体（Windows 本机 `set SHELL=cmd.exe` / `set SHELL=powershell.exe` 后 cargo run -p app）→ 窗口正常起壳（请求 1 回落路径的运行级确认，Linux 侧无法自动验证）。

## 豁免记录

- 豁免内容（已验证事实）：用户明示豁免"不得编辑 Windows 专属文件"限制，仅限一处——crates/pty/tests/pty_windows.rs 头部加 `#![cfg(windows)]`（与 pty_unix.rs 的 `#![cfg(unix)]` 对称），不做其他任何修改。
- 提交 hash：门控本体 b092fc7（信息 "shared: gate pty_windows.rs with cfg(windows) (user-approved exemption)"）；登记 docs/DECISIONS.md 49c61bc。
- Windows 复验注意事项（工程判断）：merge origin/main 后该文件头部已含门控；若 Windows 侧后续编辑覆盖文件头部，需保留 `#![cfg(windows)]` 行；豁免仅此一处，不构成先例。

## 仓库状态实测

以下为 5405372（本文档提交）推送后的实测；本节为文档自指补记，补记提交本身会推进 main 顶端，顶端 hash 以推送后 ls-remote 为准。

```
$ git log --oneline -n 12
5405372 deliver: PHASE1-MERGE.md (win phase1 merge, 3 requests, Linux acceptance, exemption record)
be5460d fix: clippy in render blit tests (field_reassign_with_default, manual_contains)
9cb99a8 request 3: render blit clips to cell rect on all four edges + unit tests
53339c3 request 2: render bg-cell assertion per-pixel by (row*width+col)
8291c81 request 1: platform default shell fallback as testable fn
2fd6013 docs: DECISIONS 登记门控提交拆分（竞态补记）
b092fc7 shared: gate pty_windows.rs with cfg(windows) (user-approved exemption)
49c61bc shared: gate pty_windows.rs with cfg(windows) (user-approved exemption)
9369c72 merge: win phase1 (Windows pty/build/scripts)
24c1333 docs: PHASE1-WIN.md 阶段1 Windows 交付记录
0446484 verify-win: Windows 阶段探针、runner 与实测日志（含 Ctrl-C E1-E8、render 帧图、app 起窗）
62f5db2 windows: ConPTY pty 实现 + 构建脚本 + rustflags + T3/T5/T6 测试 + Ctrl-C 实测数据（未解决按规格记录）

$ git status
On branch main
Your branch is up to date with 'origin/main'.
nothing to commit, working tree clean

$ git ls-remote origin
5405372a2b7ad8742bad17630395a175d264553c	HEAD
5405372a2b7ad8742bad17630395a175d264553c	refs/heads/main
24c133312bf96dff72caf1997be1964148e28274	refs/heads/win
be5460d5b99b531e4aedda3eac353393283127e2	refs/tags/phase1-merged
8e2dbdc38b0e336b21264520e8ee6f2ef5a80ea0	refs/tags/skeleton-ready
0a1e58df8534cfee8fb8d11de42faa4010fe49ce	refs/tags/verify-baseline
```
