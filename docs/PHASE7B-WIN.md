# PHASE7B-WIN：合并 7A / 0.2.0 重建 / -e argv / --working-directory / 回归 / 打包

- 日期：2026-11-18（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 输入注入不可用（无前台通道），全部结论经 `-e` 启动的子进程写标记文件取证；`.ps1` 驱动脚本用
  `ProcessStartInfo.ArgumentList`（.NET 8，经 pwsh）传参，避免 `Start-Process` 对非 ASCII 参数的破坏。

## B1 合并

`git ls-remote origin` 实际输出（节选）：

```
ae3af654da886f3e92aca9cdb824e449a568e5c7	refs/heads/main
2d05b43b6845f2d3af2ef65139cacf15f3bcf765	refs/heads/win
ae3af654da886f3e92aca9cdb824e449a568e5c7	refs/tags/phase7a-done
```

- `phase7a-done` 与任务书 `ae3af654…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- 合并冲突仅 `docs/DECISIONS.md`（双方文件尾追加）：按"双方保留"解决（6D D1–D4 / 7A 条目在前，6E 三条在后）。
  `README.md` / `README.zh-CN.md` 自动合并成功：7A 的两个新选项与示例、以及 win 上的 pwsh 条目（现象 / 规避 /
  "尚未确认是否仅在 leantty 中出现"）**都在**，两份结构一致（选项表、示例、故障排查第 6 条位置相同）。
- 合并提交 `aa48889`。
- `crates/` 相对 `phase6d-win-fix`（`9575229`）差异文件清单：

| 文件 | 变化 | 内容 |
|---|---|---|
| `crates/app/src/cli.rs` | +187 | 7A 的 `-e/--command`、`--working-directory` 解析与单测 |
| `crates/app/src/main.rs` | +42 | 目录校验 / `set_current_dir` / 传 command |
| `crates/app/src/lib.rs` | +39 | `App::new_with_command` + `exec_command` 走 spawn |
| `crates/pty/src/unix.rs` | +8 | 显式 `cmd.cwd(current_dir)`（替代 `/tmp` 硬编码） |

结论（已验证事实）：4 个文件均随合并进入，Windows 侧未自行改动 `crates/`（B6 的 cfg(windows) 修复见下）。

## B2 重建与门禁

- `scripts/windows/build.ps1 … --release`：rc=0，`build.ps1 OK`。
- `--version` 原始输出：`leantty 0.2.0`（rc=0）。
- `cargo test --workspace`（修复前）：**235 passed / 0 failed**；（B6 修复后）**236 passed / 0 failed**。
- `cargo clippy --workspace --all-targets -- -D warnings`（windows-gnu 宿主）：**rc=0、0 error**。

## B3 `-e` 的 argv 往返（已验证事实，9 例全过）

探针 `E:\dev\7b\argvprobe\probe.exe`（rustc 直编译，仓库外、无依赖）把 argv 逐项写文件；每例的
`PROBE_OUT` 标记文件存在即"子进程确实被启动"的证据。

| 例 | 期望 argv | 实测（标记文件原文） | 判定 |
|---|---|---|---|
| plain | `alpha`,`beta` | `argc=2`，`alpha`(5) / `beta`(4) | 一致 |
| spaces | `a b`,`c  d` | `argc=2`，`a b`(3) / `c  d`(4) | 一致 |
| quotes | `say "hi"`,`x"y` | `argc=2`，`say "hi"`(8) / `x"y`(3) | 一致 |
| backslash | `C:\tmp\x`,`ends\`,`C:\a b\` | `argc=3`，`C:\tmp\x`(8) / `ends\`(5) / `C:\a b\`(7) | 一致（含以反斜杠结尾） |
| empty | `""` | `argc=1`，`<>`(0) | 一致 |
| cjk | `中文参数`,`含空格 中文` | `argc=2`，`中文参数`(4) / `含空格 中文`(6) | 一致 |
| dash | `-x`,`--flag=1` | `argc=2`，`-x`(2) / `--flag=1`(8) | 一致 |
| after_e_cfg | `--config-file`,`foo.toml` | `argc=2`，原样两项 | 一致（`-e` 之后不再被 leantty 解析） |
| mixed | `a"b\`,`"`,`中 文\` | `argc=3` 原样 | 一致 |

无差异项。每例 leantty 均在子进程退出后自行退出（rc=0）。

## B4 `--working-directory`（**发现并修复 Windows 缺陷**）

### B4.1 修复前（已验证事实，红）

`--working-directory DIR -e probe` 下，探针报告的 `cwd` 恒为 **`C:\Users\Admin`**（即 `$HOME`），
既不是启动目录也不是目标目录 → Windows 上"默认=启动目录"与 `--working-directory` **双双失效**。

根因（已验证事实）：`portable-pty 0.8.1` 的 `CommandBuilder` 在 `cwd=None` 时把子进程目录设为
`$HOME`（与 7A 在 Linux 上查明的同一机制；Unix 侧 7A 已显式 `cmd.cwd(current_dir())`，Windows 侧没有对应处理）。

### B4.2 修复（`shared:` 提交 18a19de，cfg(windows) 最小改动）

`crates/pty/src/windows.rs::spawn_inner` 在 `CommandBuilder` 上显式
`cmd.cwd(std::env::current_dir()?)`（取不到就沿用旧行为），并新增回归测试
`crates/pty/tests/pty_windows.rs::spawn_child_inherits_process_cwd`（Windows 专属测试文件）。

- **修复前测试为红**：`test result: FAILED`（子进程输出 `c:\users\admin>`，断言的是临时目录）。
- **修复后为绿**：`test spawn_child_inherits_process_cwd ... ok`。

### B4.3 修复后实测（已验证事实）

| 例 | 目标目录 | 探针 `cwd` | leantty rc |
|---|---|---|---|
| normal | `E:\dev\7b\dirs\normal` | 同左 | 0 |
| space | `E:\dev\7b\dirs\with space` | 同左 | 0 |
| cjk | `E:\dev\7b\dirs\中文目录` | 同左 | 0 |
| relative | `dirs\normal`（相对启动目录 `E:\dev\7b`） | `E:\dev\7b\dirs\normal` | 0 |
| nonexist | `E:\dev\7b\dirs\nope` | 未启动子进程 | **1**，stderr `leantty: cannot use working directory '…\nope': … (os error 2)` |
| notdir | `E:\dev\7b\dirs\afile.txt` | 未启动子进程 | **1**，stderr `leantty: not a directory: …\afile.txt` |
| UNC | `\\localhost\C$\Windows` | 未启动子进程 | 1，stderr os error 67（`找不到网络名`） |

- `cwd` 输出无 `\\?\` 前缀（均为普通 `E:\…` 路径）。
- `cmd.exe` 在该目录可运行：`--working-directory "E:\dev\7b\dirs\with space" -e cmd.exe /d /c probe.cmd`
  → 脚本内 `cd > cd_out.txt`，文件内容 `E:\dev\7b\dirs\with space`（rc=0）。
- **UNC**：本机 `\\localhost\C$\Windows` 不可访问（os error 67），"UNC 可用目录"路径**未测**；
  仅证明"不可用的 UNC 会以 rc=1 + 路径报错退出、不开窗"。

## B5 回归（已验证事实）

| 项 | 结果 |
|---|---|
| `--version` | `leantty 0.2.0`，rc=0 |
| `--help` | 17 行，rc=0 |
| `--config-file` 缺值 / 不存在文件 | rc=2 / rc=1 |
| `-e` 缺程序 | rc=2，`leantty: option '-e' requires a value` |
| `--working-directory` 缺值 / 重复 | rc=2 / rc=2，`… may only be given once` |
| `-e probe`（子进程立刻退出） | leantty **712 ms** 后退出，rc=0，标记文件存在 |
| 控制台释放（`-e cmd.exe /k …` 常驻） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0** |
| `-e` 存在时 shell.args 不注入 | 子进程命令行原文 `C:\Windows\system32\cmd.exe /k "echo CONSOLE"`（**无 `chcp`**） |
| shell.args 默认（无 `-e`） | `cmd.exe /K "chcp 65001"`；`program="pwsh"` → `"…pwsh.exe"`（无 `/K`） |
| 退出检测（无 `-e`，shell 自退） | `cmd /D /K "ping -n 3 … & exit"` → leantty **2033 ms** 退出 |

## B6 是否需改 crates（汇报项，已执行）

- **需要**，理由与范围：缺陷位于 `crates/pty/src/windows.rs`（Windows 专属文件的 `spawn_inner`），
  只需一行显式 cwd 设置；`cfg(windows)` 文件内改动、无跨平台行为变化，提交信息以 `shared:` 开头（**18a19de**），
  并按任务要求"修复前先变红"（`spawn_child_inherits_process_cwd` 红 → 绿）。
- 未新增配置项、未新增依赖。

## B7 打包（leantty 0.2.0）

```
zip:  vt-verify-win/leantty-0.2.0-windows-x86_64.zip
size: 5 791 390 bytes
sha256: 01faa5e72812af78633fece06f529a15b02146fcd996de0d4e802a0889714c11
```

- 单顶层目录 `leantty-0.2.0-windows-x86_64/`，10 条目：`leantty.exe`（12 797 952 / `9ee7a93c…`）、
  `conpty.dll`、`OpenConsole.exe`（sha256 固定 + Authenticode）、`leantty.toml`、`leantty-portable.cmd`、
  `LICENSE`、`THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- **zip 内文档 vs 仓库逐文件哈希（raw 与去 CR 后均相同，无 CRLF 差异）**：

| 文件 | 仓库 raw | zip raw | 去 CR 后 |
|---|---|---|---|
| README.md | `c016c4e2acfd4a25…` | 同 | 同 |
| README.zh-CN.md | `3241c22326eff2f0…` | 同 | 同 |
| CHANGELOG.md | `35260cb51694e7e3…` | 同 | 同 |
| LICENSE | `45c88805fba0e684…` | 同 | 同 |
| THIRD-PARTY-NOTICES.md | `cd28e4b0c6324e26…` | 同 | 同 |

- 泄漏扫描（exe + zip 内 10 文件）：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root/` / `/home/` **全 0 命中**。

## 已知限制

1. **UNC 可用目录未测**（本机 `\\localhost\C$` 不可访问，os error 67）；仅测"不可用 UNC → rc=1 + 路径报错"。
2. 键盘/输入注入仍不可用：`-e` 的交互程序（如 pwsh）键盘输入、IME 均**未测**。
3. `-e` 语义为"其后全部参数原样传给程序"，因此 `-e prog --working-directory X` 会把该选项交给程序本身
   （7A 既定语义，非缺陷；本报告已在 B4 用正确顺序 `--working-directory X -e prog` 测试）。
4. pwsh 7.6.6 更新横幅崩溃问题仍按 6D/6E 结论处理（与终端尺寸无关）。

## 手动项（≤3，仅无法自动覆盖的）

1. 解压 zip 后用 `leantty-portable.cmd` 启动，并做一次真实输入法测试。
2. 在 pwsh 中执行一条输出中文的命令（如 `Write-Output '中文测试'`）。
3. 用 `-e pwsh` 运行一个交互程序，确认键盘输入正常。
