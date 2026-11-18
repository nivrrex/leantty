# PHASE6E-WIN：按用户机新信息修订 README 的 pwsh 条目（无代码改动）

- 日期：2026-11-18（按系统时间）
- 触发：用户机反馈——不设 `POWERSHELL_UPDATECHECK` 时在 leantty 的 cmd 里运行 pwsh 仍崩；设 `POWERSHELL_UPDATECHECK=Off` 后正常。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## W1 README 修订（已验证事实：文件已改）

`README.md` / `README.zh-CN.md` 的 Troubleshooting / 故障排查 第 6 条，两份结构一致，三段：

1. **现象**：在 leantty 的 shell 里运行 `pwsh` 立即失败，报
   `System.ArgumentOutOfRangeException: totalWidth ('-17')`，由 `String.PadRight` 抛在
   `UpdatesNotification.ShowUpdateNotification`（PowerShell 自己的更新提示横幅）。
2. **规避**：启动前设 `POWERSHELL_UPDATECHECK=Off`——**已在反馈问题的用户机器上确认有效**。
3. **限制**：是否只在 leantty 中出现**尚未确认**；conhost / Windows Terminal 对照目前未测。

未改 `crates/`，未新增配置项，未把环境变量注入默认值。

## W2 conhost / Windows Terminal 对照（本机）

| 目标 | 方法 | 原始输出 / 结果 |
|---|---|---|
| 普通 conhost | 自建 `.cmd`（`set POWERSHELL_UPDATECHECK=` 清空后运行 `pwsh -NoProfile -NoExit`，stderr 重定向到文件），`Start-Process cmd.exe` 启动 | `cmd_pid=16396`；15 s 后 `pwsh_child=16820 alive=yes`；`ready=yes`；`exited` 文件不存在；`err.txt` 为空 → **未复现崩溃** |
| Windows Terminal | `Get-Command wt.exe` | `wt=absent`（本机未安装）→ **未测** |

- 处置：conhost 未复现，Windows Terminal 未测；因此 README 中"是否只在 leantty 中出现"保持"尚未确认"。
- 进程处置：只结束自己 `Start-Process` 启动并核对过命令行的 `cmd.exe`（PID 16396）；**未**结束任何
  `pwsh`/`OpenConsole`/`conhost`。该 cmd 被结束后其 pwsh 子进程可能短暂残留，按红线**只记录、不处理**。

## W3 重打 zip 与收口

| 项 | 结果 |
|---|---|
| zip | `leantty-0.1.1-windows-x86_64.zip`，**5 786 563 B**，sha256 `6203b6663999913014eae0c34549cc8e317c9d7051eff9355cd6001f58386682` |
| zip 内文档哈希 vs 仓库 | README.md `e48411bf…`、README.zh-CN.md `21322922…`、CHANGELOG.md `68903d9f…`、LICENSE `45c88805…`、THIRD-PARTY-NOTICES.md `cd28e4b0…` —— **五项全部一致** |
| 泄漏扫描 | exe 与 zip 内 10 个文件：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root` / `/home` **全部 0 命中** |
| 代码改动 | **无**（仅 README 两条 + 本报告 + DECISIONS） |

## 待验证假设

- `POWERSHELL_UPDATECHECK=Off` 的规避在用户机有效（用户已确认），但崩溃的**确切触发条件**仍未知
  （本机 conhost 与 leantty 均未复现）。推翻条件：在用户机设 `Off` 后仍崩溃 → 需重新归因到 ConPTY 的
  其它初始化差异。

## 手动项（≤3）

1. 在 pwsh 中执行 `Write-Output '中文测试'`，确认中文编码。
2. 在 leantty 内做一次真实输入法测试（候选窗跟随、提交文本）。
3. 如装有 Windows Terminal，在其中的 pwsh 里复测一次（本机未安装，未测）。
