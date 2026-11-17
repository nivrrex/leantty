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
