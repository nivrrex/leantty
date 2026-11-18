# PHASE6D-WIN：pwsh 7.6.6 启动崩溃的取证与处置（无代码改动）

- 日期：2026-11-18（按系统时间）
- 触发：6C 手动测试发现——在 leantty（shell = cmd）里运行 pwsh 7.6.6 立即崩溃：
  `System.ArgumentOutOfRangeException: totalWidth ('-17') ... at String.PadRight ... at UpdatesNotification.ShowUpdateNotification`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## W1 取证（全部自动执行；注入通过 `terminal.shell.args` 驱动，不依赖键盘注入）

### W1.1 ConPTY 报告的尺寸 vs 配置网格（已验证事实）

探针：`shell.args = ["/D","/K","E:/dev/6d/a/probe.cmd"]`，脚本输出 `mode con` 与
`$Host.UI.RawUI` 到文件，并写 `sentinel.txt` 作为"命令确实执行"的哨兵。

| 项 | 实测 |
|---|---|
| `mode con`（在 leantty 内） | 行 **30**、列 **120** |
| `$Host.UI.RawUI.WindowSize` | **120,30** |
| `$Host.UI.RawUI.BufferSize` | 120,30 |
| `$Host.UI.RawUI.MaxPhysicalWindowSize` | 1904,62 |
| leantty 配置网格 | `dimensions = { columns = 120, lines = 30 }` |
| 哨兵 | `sentinel.txt` 存在（命令确实在 ConPTY 里执行） |

→ ConPTY 初始尺寸**正确**，不是 0，也不是过小。

### W1.2 windows.rs 尺寸来源与 resize 到达（已验证事实 + 代码阅读）

- `crates/pty/src/windows.rs::spawn_inner` 用调用方传入的 `cols/rows` 建 `PtySize`（app 传
  `config.cols/rows` = 120/30），并对 0 值显式报错；**不存在**"默认值/0 提前使用"的路径。
- 首次 resize 到达：对运行中的 leantty 窗口 `SetWindowPos` 到 800×500 后，`LEANTTY_PERF_FRAMES=1`
  日志出现 `frames_resize=1`（此前为 0）→ resize 事件被接收并走到 `pty.resize`。

### W1.3 可复现性排查（已验证事实）

| 场景 | 结果 |
|---|---|
| leantty（cmd）→ `pwsh -NoProfile -Command …` | **正常**：`EXITCODE=0`，`7.6.6|120,30`，哨兵生成 |
| leantty（cmd）→ `pwsh -NoProfile -NoExit -Command …`（交互 prompt） | 12 s 内**无崩溃**（`ready.txt` 有、`exited.txt` 无） |
| leantty（cmd）→ `pwsh`（无参数交互） | 30 s 内**无崩溃**（`crashed.txt` 不存在，pwsh 子进程存活） |
| 同上 + 期间最小化窗口（SW_MINIMIZE） | 仍**无崩溃** |
| leantty 直接 `terminal.shell.program = "pwsh"` | 9 s 后 pwsh 子进程存活（`"C:\Program Files\PowerShell\7\pwsh.exe"`） |
| 列宽 80 / 60 / 40 / 25（交互 pwsh） | 四档均**无崩溃**（`ready.txt` 有、`exited.txt` 无） |
| **对照：普通 conhost 120×30** 里 `mode con cols=120 lines=30` 后跑交互 pwsh | 20 s 后 pwsh 子进程存活、**无崩溃**（与 leantty 行为一致） |
| 网络/更新源 | GitHub API 可达（200），latest = **v7.7.0** > 本机 7.6.6（即"存在可用更新"） |

- 本机 `POWERSHELL_UPDATECHECK` 未设置（默认）。`POWERSHELL_UPDATECHECK=Off` 的规避路径因此无法
  通过"崩溃消失"来正向验证（本来就没有复现崩溃）→ 该规避项标 **未验证其必要性**。
- 所有启动的 leantty 实例均按**记录 PID + 命令行核对**结束后再继续；未按名字结束
  pwsh/OpenConsole/conhost（其中 conhost 对照实验的 cmd 也按记录 PID 结束）。

## W2 定位结论

- **已验证事实**：ConPTY 尺寸、初始值、resize 更新均正确；leantty 与同宽度 conhost 行为一致；
  9 种组合下 pwsh 7.6.6 均未崩溃。
- **工程判断（置信度中）**：崩溃栈全部位于 pwsh 自身的更新提示横幅代码
  （`UpdatesNotification.ShowUpdateNotification` → `String.PadRight(totalWidth)`，`totalWidth = -17`），
  且"存在可用更新（v7.7.0）"这一前提成立；终端侧尺寸因素已被实测排除 → **属 PowerShell 7.6.6
  的缺陷，不是 leantty 的 ConPTY 尺寸问题**。按 W2 第 2 条：**不改代码**。
- **待验证假设**：触发崩溃的确切条件（横幅在某类宽度/缓冲状态下的宽度计算）未复现；需在用户机上
  用 `POWERSHELL_UPDATECHECK=Off` 复测确认规避有效。推翻条件：设 `Off` 后仍崩溃 → 需重新归因，
  转向 ConPTY 的其它初始化差异（TERM/启动序列）。

### 处置（用户可见，不改默认行为）

- `README.md` / `README.zh-CN.md` 的 Troubleshooting / 故障排查 各加一条：现象、`POWERSHELL_UPDATECHECK=Off`
  规避、以及"未能复现"的限制说明。**未**把该环境变量注入默认配置或代码（属产品行为变更，需先授权）。

## W3 收口

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **233 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **0 error** |
| 重打 zip（README 变更后） | `leantty-0.1.1-windows-x86_64.zip`，**5 786 634 B**，sha256 `1713d3f5c01a5348cb387956a392ff7d69aba44264ff23230e6b2c79a829132a` |
| zip 内文档哈希 vs 仓库 | README.md `da487e9e…`、README.zh-CN.md `6560c9ea…`、CHANGELOG.md `68903d9f…`、LICENSE `45c88805…`、THIRD-PARTY-NOTICES.md `cd28e4b0…` —— **五项全部一致** |
| 泄漏扫描 | exe 与 zip 内 10 个文件：仓库路径/`\Users\`/主机名/`ali:8022`/`/root`/`/home` **全部 0 命中** |
| 代码改动 | **无**（仅 README 两条 + 本报告 + 日志）；`crates/` 未动 |

## 手动项（≤3，仅无法自动覆盖的）

1. 在 pwsh 中执行 `Write-Output '中文测试'`，确认中文编码正确。
2. 在 leantty 内做一次真实输入法测试（候选窗跟随、提交文本）。
3. 在你复现崩溃的机器上按 README 新增条目设置 `POWERSHELL_UPDATECHECK=Off` 后重启 pwsh，确认崩溃消失（用于验证上面的待验证假设）。
