# PHASE5B-WIN：合并 5A / leantty 回归 / CLI 实机 / 控制台处理（停止于 t4）

- 日期：2026-11-17（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- **本阶段在 t4 触发停止条件**：`Start-Process` 新控制台启动后，应用仍持有自己的控制台窗口
  （5A t4 的 FreeConsole 预期不成立）。按规则"与预期不符：停止汇报，附证据；不要自行改 crates/"，
  t5（图标嵌入）、t6（路径泄漏扫描）、t7（zip）**未执行**，未打 tag。

## 1. t1 合并 5A

| 项目 | 结果 |
|---|---|
| `phase5a-done` | `0b7115ef483290e7802930fca35673936c4d2639`（与 `refs/heads/main` 同值；**轻量 tag，无 `^{}` 行**，解引用同值） |
| 合并 | `--no-ff` 合并为 `3bb9737`，0 冲突（132 文件，+5482/−1156） |
| 改名提交 | `f44d639 rename: 项目改名 leantty（二进制/包名/环境变量/配置目录/窗口标题）`；`bb070ad rename: 脚本内的 cargo -p 包名同步` |
| 改名是否只含标识符/字符串替换 | 是（已验证）：`git show f44d639 -- crates/` 中删除行全部是 `APP_*`/`app` 命名，替换为 `LEANTTY_*`/`leantty`；Windows 专属文件（`crates/pty/src/windows.rs`、`pty_windows.rs`）与 `scripts/windows/**` 的改动同为名字替换（例：`target\debug\app.exe` → `target\debug\leantty.exe`、`APP_DUMP_SNAPSHOT` → `LEANTTY_DUMP_SNAPSHOT`） |

## 2. t2 构建与回归

| 项 | 结果 |
|---|---|
| `scripts/windows/build.ps1 --release` | 通过（11m16s，0 warning），产出 `target\release\leantty.exe`，**12 683 264 B** |
| `cargo test --workspace` | **228 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **失败 1 处（既有代码）**：`crates/app/src/lib.rs:113` doc list item without indentation（5A 引入的 doc 缩进；crates 不得改 → 记请求） |

### 2.1 shell.args 五例（新二进制/新环境变量）

| 例 | 子进程命令行（原文） | 判定 |
|---|---|---|
| a 无 `[terminal.shell]` | `C:\Windows\system32\cmd.exe /K "chcp 65001"` | 默认一致 |
| b `program="cmd.exe"` | `C:\Windows\system32\cmd.exe /K "chcp 65001"` | 同 a |
| c `program="cmd.exe"` + `args=["/D","/K","echo X>…"]` | `C:\Windows\system32\cmd.exe /D /K "echo X>E:/dev/5b/marker.txt"`，marker=True | 用户 args 生效、无 chcp |
| d `program="pwsh"` | `"C:\Program Files\PowerShell\7\pwsh.exe"` | 无 `/K` |
| e `args=[]` | `C:\Windows\system32\cmd.exe` | 显式空数组不注入默认 |

### 2.2 退出检测与空闲唤醒（新二进制）

| 场景 | 结果 |
|---|---|
| 子 shell 自行退出（pwsh 3 s 后 exit，`done.txt` 标记退出时刻）× 3 | 延迟 **1258 / 1319 / 2196 ms**（≤5 s，均关闭） |
| 直接杀子 shell（记录 PID + 命令行核对）× 3 | **1787 / 1780 / 1954 ms** 关闭 |
| 空闲 30 s（cmd `/K echo ok`，`LEANTTY_PERF_FRAMES=1`，直接句柄重定向） | 7 条 5 秒统计行；`wake_timer` = 0（启动）、15（启动期）、5×5 → 稳态 **≈1 次/秒** |

## 3. t3 命令行参数（实机，直接句柄重定向）

| 用例 | 实际输出 | 退出码 |
|---|---|---|
| `--version` | `leantty 0.1.0` | **0** |
| `--help` | `Usage: leantty [OPTIONS] ...` | **0** |
| `--nope` | stderr `leantty: unknown option '--nope'` + `Try --help` | **2** |
| `--config-file`（缺值） | stderr `leantty: option '--config-file' requires a value` | **2** |
| `--config-file 不存在路径` | stderr `leantty: 无法读取配置文件 'E:/dev/5b/nope.toml': 系统找不到指定的文件。` | **1** |
| 优先级（`LEANTTY_CONFIG=env.toml`，`--config-file cli.toml`，两文件各含不同未知键） | stderr 仅 1 条：`config: 未知字段 \`keyboard.bindings[1].zzz_cli_unknown\`，已忽略`；`zzz_env_unknown` 计数 0 | — |

结论：六项均符合预期（命令行配置优先于 `LEANTTY_CONFIG`）。

## 4. t4 控制台处理（**停止点**）

### 4a Start-Process 新控制台（已验证事实，与预期不符）

- `EnumWindows` 按 PID 过滤的**可见 `ConsoleWindowClass` 窗口数量 = 1**：
  `pid=6072 title=[E:\dev\tty\target\release\leantty.exe]`（窗口线程 PID 即 app 自身）。
- 独立探针 `AttachConsole(app_pid)` **返回 True**，`GetConsoleWindow()≠0` → 应用仍在某个控制台上。
- 进程树：`conhost.exe` 的父进程 = app pid（例：`conhost 5408 ← leantty 6524`）→ 控制台确实属于 app。
- 预期（5A t4）："当前进程是控制台唯一使用者时 FreeConsole，双击启动不再残留黑框" → 本机**不成立**。
- 退出检测未回归：同一次运行中杀掉 app 的 ConPTY 子 shell 后 app **1988 ms** 退出。

### 4b `cmd /c "leantty.exe 2>file"`（未测）

- 本轮该子项的启动方式未成功（bash→cmd 的嵌套引号传参使 cmd 报"不是内部或外部命令"，err.log 是 cmd 的错误而非 app 输出）；
- 且 4a 已显示控制台未被释放，"控制台保留 + 重定向正常"的前提无法独立成立 → 标 **未测**。

### 4c 处置

按任务书停止条件：**停止，不自改 crates/**。证据：`vt-verify-win/logs/5b/t4-evidence.txt`。
可检验的候选（供 5C/Windows 后续裁定，均需改 crates）：
1. `GetConsoleProcessList(null,0)` 的返回值语义判断（是否把"查询失败 0"与"1 个使用者"混淆）；
2. FreeConsole 后是否被 ConPTY/portable-pty 的 spawn 重新附加/新建控制台；
3. 若设计上要求"仅 .cmd/双击场景释放"，需在报告中给出判定条件（当前实现只看进程数）。

## 5. 未执行项（停止条件触发）

| 任务 | 状态 | 原因 |
|---|---|---|
| t5 图标嵌入 exe | **未做** | t4 停止 |
| t6 路径泄漏扫描 | **未做** | t4 停止 |
| t7 zip 产出与验证 | **未做** | t4 停止 |

## 6. 请求栏（需 Linux 侧/授权处理，本任务不改 crates/）

1. **控制台释放未生效**（见 §4a）：需按 §4c 的候选定位；本机可复现脚本 `E:\dev\5b\t4d.ps1`、`t4probe.ps1`。
2. **clippy `-D warnings` 在 Windows 上失败**：`crates/app/src/lib.rs:113` doc 缩进（5A 引入），影响门禁。
3. `scripts/windows/e2e/*.ps1` 仍硬编码 `$ROOT = 'C:\dev\tty'`（5A 未改；3T 只修了 `wb2_lib.ps1`），本轮未动。

## 7. 交付

- 提交：`bc0b7de`（合并 5A）、本报告与证据日志。**未打 tag**（t4 停止，阶段未完成）。
- 未改 `crates/`；未产出 zip。

---

# 5B 续（控制台修复 / 图标 / 泄漏扫描 / RC zip）

- 起点：`git status` 干净，`git log -3` = `1b6c292` / `650c0f6` / `3bb9737`（未改写任何既有提交）。

## 7. t0 clippy（已验证事实）

- 起点唯一告警：`error: doc list item without indentation --> crates\app\src\lib.rs:113:9`（5A 引入）。
- 修复：该 doc 列表中插入空行（`shared:` 提交 **e476f8a**）；修后 `cargo clippy --workspace --all-targets -- -D warnings` **rc=0、0 error**（`vt-verify-win/logs/5b-clippy.log`）。
- 无其它告警。

## 8. t4a 探针（rustc 直接编译，`vt-verify-win/probe_console5b/`，不入库）

程序（`probe.rs`）记录结果到文件，不依赖 stdout；三种启动方式各跑一次：

| 观测 | Start-Process 新控制台 | .cmd 内直接运行（共享） | cmd `start ""` 新窗口 |
|---|---|---|---|
| 释放前可见 ConsoleWindowClass（本 PID） | 1 | 0（窗口属 cmd） | 1 |
| `GetConsoleProcessList(null,0)` | **0，GetLastError=87（ERROR_INVALID_PARAMETER）** | 同 | 同 |
| `GetConsoleProcessList(buf,8)` | **1，pids=[self]** | **2，pids=[self, cmd]** | 1 |
| `FreeConsole()` | **1（成功）** | 1 | 1 |
| 释放后窗口（+1s / +3s / +8s） | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| 释放后 `AttachConsole(self)` | 0（失败，GetLastError=6） | 0 | 0 |

**结论：候选原因 #1 成立**——空缓冲查询形式在本机返回 0 并置 `ERROR_INVALID_PARAMETER`，
`should_free_console(0)=false`，因此 5A 的释放分支从未执行；传非空缓冲（容量 2）才能拿到真实进程数
（独占=1、共享=2），FreeConsole 本身工作正常。

## 9. t4b 修复（`shared:` 提交 f8eef9b）

`crates/app/src/lib.rs::console::free_if_sole_user`：`GetConsoleProcessList(null,0)` →
`GetConsoleProcessList(buf.as_mut_ptr(), 2)`（`let mut buf = [0u32; 2]`），其余不变
（`should_free_console` 纯函数、失败忽略、调用点仍在参数解析成功且非 help/version 之后、事件循环之前）。

## 10. t4c 验证（修复后二进制）

| 项 | 结果 |
|---|---|
| a. 新控制台启动：本 PID 可见 ConsoleWindowClass 数量 | **1s=0 / 3s=0 / 8s=0** |
| a. 独立探针 `AttachConsole(app)` | **False**（不再持有控制台） |
| a. 应用窗口 | 存在（`Window Class` + IME 窗口，共 2 个可见窗口） |
| a. ConPTY 子 shell 正常工作 | 子进程 `cmd.exe /D /K "echo X>…marker.txt"`，**marker=True** |
| a. 子 shell 退出检测 | 杀子 shell 后 app **2044 ms** 退出（≤5 s） |
| a. 空闲 30 s 唤醒 | 7 条 5 秒统计行，`wake_timer=[0,14,5,5,5,5,5]` → 稳态 **≈1 次/秒** |
| b. `.cmd` 内 `leantty.exe 2>file`（共享控制台） | cmd 的控制台窗口 **保留**（cmd pid 计数=1），app 自身控制台窗口 **0**；`ver.txt`=`leantty 0.1.0`；`err.log` 5450 B（perf 行）；批处理继续执行（`after.txt`=`AFTER`） |
| c. `--version` 在共享控制台运行 | 输出被重定向捕获为 `leantty 0.1.0`，cmd 未中断（同上 `AFTER`） |

证据：`vt-verify-win/logs/5b/t4-evidence-2.txt`。

## 11. t5 exe 图标（已验证事实）

- 新增 `assets/icon/leantty.rc`（`1 ICON "leantty.ico"`）与 `scripts/windows/gen-icon-res.ps1`：
  windres → `target/windows/leantty-res.o`（109 850 B，PE-x86-64 COFF）→
  `cargo rustc -p leantty-app --bin leantty -- -C link-arg=<obj>`。
- **与任务描述的偏离（有实测依据）**：资源对象不能写进 `.cargo/config.toml` 的
  `[target.x86_64-pc-windows-gnu] rustflags`——该段同时作用于 build script / proc-macro 的宿主链接，
  实测 `collect2.exe: error: ld returned 1 exit status`（`thiserror` build script 链接失败）。
  故改为"构建时由脚本生成对象 + `cargo rustc` 只把 link-arg 附加给最终 bin"；config 仅保留
  `--no-gc-sections` 与说明注释。
- 验证（`ExtractAssociatedIcon`）：`leantty.exe` 图标 32×32、sha256 `60DDA1CA…`；
  同工具从**无资源**的 rustc 产物 `probe.exe` 取到默认图标 32×32、sha256 `575EF706…` →
  **不是系统默认通用图标**（且与 `assets/icon/leantty.ico` 同源）。
- 提交 **292beb4**（build.ps1 调用链 + gen-icon-res.ps1 + leantty.rc + .cargo/config.toml 注释）。

## 12. t6 路径泄漏扫描（已验证事实）

- build.ps1 增加 `RUSTFLAGS`：`--remap-path-prefix=<repo>=/leantty`、`<CARGO_HOME>=/cargo-home`、
  `<RUSTUP_HOME>=/rustup-home`（三者运行时取值；本机 `CARGO_HOME=E:\dev\rust\cargo`、
  `RUSTUP_HOME=E:\dev\rust\rustup`），并显式保留 `--no-gc-sections`（RUSTFLAGS 会覆盖 config 的
  `[target.*] rustflags`）。全量重建 135 个 crate，rc=0。
- 扫描 `leantty.exe`（`strings -n 6`，38 160 行）与 zip 内全部 9 个文件：

| 模式 | 命中 |
|---|---|
| 仓库路径 `E:\dev` / `dev\tty` | **0** |
| 映射前缀 `/leantty/`（源码绝对路径残留） | **0** |
| `/cargo-home`（中性前缀，属预期） | 569 |
| `/rustup-home`、`ghostty-src`、`zig-system` | 0 / 0 / 0 |
| `\Users\`、用户名 `Admin` | 0 / 0 |
| 主机名 `DESKTOP-FE1AADA` | 0 |
| IPv4、`ssh://ali`、`ali:8022`、`/root/`、`/home/` | 0 / 0 / 0 / 0 / 0 |
| 盘符路径 `[A-Za-z]:\` | **3 行**（全部列出，见下） |

盘符路径 3 行（原样）：

1. `\$'F:\` —— 二进制数据碎片，非路径（无随附可读文本）。
2. `u1B:\` —— 同上，属字符串表碎片。
3. `SYSTEMROOTFontsC:\Windows\Fonts\USERPROFILEAppData\Local\Microsoft\Windows\FontsAppData\Roaming\Microsoft\Windows\Fonts`
   —— 字体目录常量（来自依赖的 fontdb 类库；`crates/`/`vendor/` 内 `grep Fonts` 0 命中），是 Windows 系统目录
   与环境变量名，**不含用户名/主机名/仓库路径**。

- 判定：**无用户/主机/仓库路径泄漏**，不触发停止条件；第 3 项属依赖内置系统路径，记录不下架。

## 13. t7 RC zip（已验证事实）

```
scripts/windows/package-release.ps1 -ConptyDir <kit v4>
zip:  vt-verify-win/leantty-0.1.0-windows-x86_64.zip
size: 5 783 314 bytes
sha256: 4c33a4593dcb66b532423d4c0b215fa26789ca346d42d1c05dc82afab9de0acb
```

- 单一顶层目录 `leantty-0.1.0-windows-x86_64/`，9 个条目：`leantty.exe`（12 789 248 / `32b6efe5…`）、
  `conpty.dll`（119 608 / `e971856f…`）、`OpenConsole.exe`（1 089 336 / `b17e48b0…`）、`leantty.toml`
  （3 448 / `623fc939…`）、`leantty-portable.cmd`（74 / `4edfa1ab…`）、`LICENSE`、`THIRD-PARTY-NOTICES.md`、
  `README.md`、`README.zh-CN.md`；无调试符号/日志/开发文档。
- ConPTY 两文件沿用既有校验：sha256 命中固定值 + Authenticode `Valid` 且签名主体含 Microsoft Corporation。
- zip 用 `IO.Compression.ZipArchive` 流式写入（未用 Compress-Archive）。
- 验证：解压到 `%TEMP%\5B 发布 验证\leantty-0.1.0-windows-x86_64`（含空格与中文），从 `C:\` 运行
  `--version` → `leantty 0.1.0`、rc=0；用 `--config-file leantty.toml` 启动后**存活 10 s**、
  **stderr 0 字节**（无未知字段/解析/字体告警，`request_inner_size` 提示未出现）、
  **随附 OpenConsole.exe 生效**（进程路径位于解压目录）、随后按句柄关闭成功。
- 说明（已验证事实）：`Start-Process -ArgumentList` 在参数含非 ASCII 路径时把参数编码弄坏
  （实测报 `unexpected argument '<乱码>'`），改用 `System.Diagnostics.ProcessStartInfo`（CreateProcessW，
  UTF-16）后正常；这是启动方式限制，不是应用缺陷（直接命令行传中文路径的 `--version` 正常）。

## 14. t8 README 核对（只列不改）

- `--version` 描述与实测一致（`leantty 0.1.0`）。
- 以下 README 陈述本轮**未复核**（不构成矛盾）：ConPTY 缺一半时回落系统 conhost 并打印警告；
  系统 conhost 下 `ESC[7m` 被改写 / 折行物化 / SGR 上报可能被吞。
- 未发现与实测冲突的 Windows 陈述。

## 15. 门禁与提交

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **228 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **0 error** |
| 提交 | `e476f8a`（clippy doc）、`f8eef9b`（控制台修复）、`292beb4`（图标）、本报告 + DECISIONS + `package-release.ps1` |
