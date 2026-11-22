# PHASE9B-WIN：合并 9A / 0.2.2 重建 / 回归 / 汉字回退重绘取证 / 打包

- 日期：2026-11-19（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## F1 合并

`git ls-remote origin` 实际输出（节选）：

```
b7567b4d102ab8452dd92190ee3ce6a91f36b254	refs/heads/main
154c6c0ab3185161e48789d4697c1df3699dfa5a	refs/heads/win
7628b13ed5d8a7ac5956eb1683d7dd19ac61a981	refs/tags/phase9a-done
```

- `phase9a-done` 与 Linux 给出的 `7628b13e…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- `--no-ff` 合并，**0 冲突**（`docs/DECISIONS.md` 本次自动合并成功），合并提交 `6db06c5`。
- `crates/` 相对 `phase8c-win-done`（`86587cb`）差异：**仅 1 个文件**

| 文件 | 变化 | 内容 |
|---|---|---|
| `crates/app/src/lib.rs` | +27/−1 | 9A：回退字体并入后置 `urgent_render` + `request_redraw`（经既有出口）；另加 debug-only 延迟钩子 `LEANTTY_TEST_DELAY_BG_FONTS_MS` |

## F2 重建与门禁

- `scripts/windows/build.ps1 … --release`：rc=0，`build.ps1 OK`。
- `--version` 原始输出：`leantty 0.2.2`（rc=0）。
- `cargo test --workspace`：**239 passed / 0 failed**。
- `cargo clippy --workspace --all-targets -- -D warnings`：**rc=0、0 error**。

## F3 回归（已验证事实，新二进制）

| 项 | 结果 |
|---|---|
| `--version` | rc=0，`leantty 0.2.2` |
| `--help` | rc=0 |
| `--config-file` 缺值 / 不存在文件 | rc=2 / rc=1 |
| `-e` 缺程序 | rc=2 |
| `--working-directory "E:\dev\7b\dirs\with space" -e probe` | rc=0，子进程 `cwd=E:\dev\7b\dirs\with space` |
| `--working-directory E:\dev\9b\nodir -e probe` | **rc=1**，stderr 含路径（os error 2） |
| 控制台释放（`-e cmd.exe /k …`） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0** |
| `-e probe`（立即退出） | leantty **479 ms** 后退出，rc=0，标记文件存在 |
| 默认配置启动 | `stderr_len=0`（无输出） |

## F4 汉字回退（重绘）取证

- **仓库现状**：该路径**没有单测**（`grep` 在测试模块内 0 命中 `urgent_render`/`bg_rx`）；9A 的端到端验证是 Linux e2e `case81`
  （debug-only 钩子 + 截图前后差异），不覆盖 Windows。
- 尝试改以单测证明：需要构造 `AppHandler`（winit 窗口 + 事件循环），超出"cfg(windows) 最小改动"的范围 → **未新增单测**。
- **Windows 实机（debug 构建，钩子可用）**：`LEANTTY_TEST_DELAY_BG_FONTS_MS=2000` + CJK fallback（Microsoft YaHei），
  shell 打印 `中文目录 临时` 后 **不再产生任何输出**（`ping -n 30 127.0.0.1 >nul`），用 `PrintWindow` 截图：

| 时间点 | 结果 |
|---|---|
| ≈2 s（字体未并入） | 该行显示为**方块**（截图 `f4-quiet-before.png`，可见 6 个方块） |
| ≈9 s（并入后） | 同一行显示为 **中文目录 临时**（`f4-quiet-after.png`） |
| 像素差 | **665 px**，包围盒 `(8,31)-(108,43)`（仅这一行文字） |

→ 在**没有新终端输出**的前提下发生了重绘，证明"字体并入 → 请求重绘"这条路径在 Windows 上生效（**debug 构建 + debug-only 钩子**）。
- **release 构建的实机冷启动**（用户最初的方块场景）**未测**：钩子仅 debug 编译，release 下无法延迟字体并入，
  冷启动时序在自动化里不可控。**不得记为已验证**。

## F5 打包（leantty 0.2.2）

```
zip:  vt-verify-win/leantty-0.2.2-windows-x86_64.zip
size: 5 793 973 bytes
sha256: f80d82fe6f5e26da7eb4f8e746a957fdbf6e7e6fbb71b771fa2355001ce70538
```

- 单顶层目录 `leantty-0.2.2-windows-x86_64/`，**10 条目**：`leantty.exe`（12 801 536 / `753bbc1c…`）、
  `conpty.dll`、`OpenConsole.exe`、`leantty.toml`、`leantty-portable.cmd`、`LICENSE`、
  `THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- **zip 内文档 vs 仓库逐文件哈希（raw 与去 CR 均相同，无 CRLF 差异）**：

| 文件 | raw sha256 | 去 CR |
|---|---|---|
| `leantty.toml` ↔ `config/leantty.windows.toml` | `8824111bd1fda252…` | 相同 |
| `README.md` | `9bd64fea263b0892…` | 相同 |
| `README.zh-CN.md` | `80bdcd63b9c992ab…` | 相同 |
| `CHANGELOG.md` | `40cfa656819ca296…` | 相同 |
| `LICENSE` | `45c88805fba0e684…` | 相同 |
| `THIRD-PARTY-NOTICES.md` | `cd28e4b0c6324e26…` | 相同 |

- 泄漏扫描（exe + zip 内 10 文件）：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root/` / `/home/` **全 0 命中**。

## 已知限制

1. release 冷启动的"方块→中文"未测（钩子 debug-only，自动化无法控制冷启动时序）。
2. 该路径无单测（需重构 `AppHandler` 才能单测，超出本任务允许的 crates 改动范围）。
3. 鼠标/键盘注入不可用：拖选、IME 仍未测；UNC 可用目录未测（沿用 7B 记录）。
4. 独立构建的 debug 产物（`target\debug\leantty.exe`，190 MB）未入库，仅用于本次取证。

## 手动项（≤3，无法自动覆盖）

1. 真实桌面冷启动观察：解压 0.2.2 zip，冷启动首次打印中文（或含中文的提示符）时，确认方块不再残留（release 行为）。
2. 解压 zip 后用 `leantty-portable.cmd` 启动并做一次真实输入法测试。
3. 真实鼠标点击与拖选手感（单击不留标记、三行流式拖动）。
