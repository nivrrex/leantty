# PHASE7A-LINUX：`-e/--command` 与 `--working-directory`（a0 现状报告后停止）

## a0 现状（已验证事实）

| 项 | 位置 | 现状 |
|---|---|---|
| CLI 解析入口 | `crates/app/src/cli.rs:53` `pub fn parse_args<I: IntoIterator<Item=OsString>>` | 纯函数；错误类型 `UnknownOption`/`MissingValue`/`UnexpectedArgument`（`cli.rs:25-29`），`Display` 各一行（`:35-37`）|
| 退出码 | `crates/app/src/main.rs:52`（用法错误 → 2）；`:117`、`:126`（显式 `--config-file` 不可读/不可解析 → 1）| help/version 返回 `Ok(())`（0）|
| 子进程创建 | `crates/app/src/lib.rs:1303` `pty.spawn_with_args(&config.shell, shell_args.as_deref(), cols, rows)` | program = `config.shell`（`terminal.shell.program` → `$SHELL` → `/bin/sh`）；args = `terminal.shell.args`（`Option<Vec<String>>`，未配置为 `None`）|
| Unix 工作目录 | **`crates/pty/src/unix.rs:65` `cmd.cwd("/tmp");`** | **显式把子进程工作目录设为 `/tmp`**（硬编码绝对路径），既不继承启动目录也不可配置 |
| Windows 工作目录 | `crates/pty/src/windows.rs` 全文无 `lpCurrentDirectory`/`cwd` 调用 | 继承父进程当前目录 |

## 停止：命中 a0 停止条件

任务书 §2 停止条件第一条为"a0 发现 Unix 或 Windows 路径在未指定时显式设置了别的工作目录（列出行号，由我裁定）"。
实际状态正是如此：**`crates/pty/src/unix.rs:65` 硬编码 `cwd("/tmp")`**。这带来两个必须先裁定的问题：

1. **行为与任务假设不符**：a2 要求"未指定 `--working-directory` 时行为与现状完全一致（子进程工作目录等于启动目录）"，
   但现状是 `/tmp`。若照现状保留，则 e2e a4 第 9 条"子进程工作目录等于启动目录"无法成立；若按任务书实现（删除该行），
   则是**产品行为变更**（用户启动目录从 `/tmp` 变成继承父进程），需要你批准。
2. **仓库禁止绝对路径**：`cmd.cwd("/tmp")` 是源码里的绝对路径，与"仓库内不得出现绝对路径"的硬性规则冲突。

因此本轮未做任何代码修改、未打 tag、未 push。工作区干净，`main` = `9783949`（= `phase6d-done`，与 ali 一致）。

## 待裁定（建议）

- **方案 A（推荐）**：删除 `crates/pty/src/unix.rs:65` 的 `cmd.cwd("/tmp")`，让 Unix 与 Windows 一致地继承启动目录；
  同时按任务书实现 `--working-directory`（在 app 侧 `set_current_dir`，从而子进程继承）。这是行为变更，需你批准；
  变更后需在 README/CHANGELOG 记为"默认工作目录由 /tmp 改为启动目录"。
- **方案 B**：保留 `/tmp` 作为默认，`--working-directory` 通过给 `spawn_with_args` 之外的新参数传递 cwd —— 需要改 Pty trait
  （任务书禁止改 Pty 既有签名），不可行。
- **方案 C**：保留 `/tmp`，`--working-directory` 只对 leantty 自身生效（用 `set_current_dir`），但子进程仍被 `/tmp` 覆盖 →
  该选项对子进程无效，与需求矛盾，不可行。
