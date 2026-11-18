# PHASE6C-WIN：合并 6B / 0.1.1 重建 / 回归 / 键映射单测 / 打包

- 日期：2026-11-17（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`，origin = `ssh://ali:8022/srv/git/tty.git`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## C1 合并

`git ls-remote origin` 实际输出（节选）：

```
3485a10b8601ef11683f2ce5bcc1e309af375776	refs/heads/main
59bdf0f0488a839e30e7020d7b8b902336ffdcfd	refs/heads/win
4b796120c1510824e5f3fc1539acaae3ee3f2449	refs/tags/phase6b-done2
```

- `phase6b-done2` 与任务书给定的 `4b796120…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- `--no-ff` 合并（无冲突）。
- `crates/` 相对 `phase5b-win-done`（`59bdf0f`）的差异清单（全部来自 Linux 6A/6B，Windows 侧未自行改动）：

| 文件 | 变化 | 内容 |
|---|---|---|
| `crates/app/src/lib.rs` | +130 | 6B 键路由：控制字符文本交编码器、`unshifted_codepoint`、ctrl+[ 特例、Shift 滚动绑定 |
| `crates/pty/src/windows.rs` | 1 行注释 | `C:\dev\tty` → `<dev-root>`（去硬编码） |
| `crates/render/tests/background_load.rs` | 5 行 | 测试细节调整 |
| `crates/vt/src/encoder.rs` | +23 | 6A/6B 编码器修复 |
| `crates/vt/tests/key_kitty.rs` | +86 | kitty 编码测试 |

结论（已验证事实）：无 Windows 侧不应出现的改动；上述均由合并带入。

## C2 重建

`scripts/windows/build.ps1 -Zig … -GhosttySourceDir … -ZigSystemDir … --release`：rc=0，`icon linked into leantty.exe`，`build.ps1 OK`。

`--version` 原始输出：

```
leantty 0.1.1
```

（rc=0）

## C3 回归（新二进制，自动执行）

| 项 | 结果 |
|---|---|
| `--version` | `leantty 0.1.1`，rc=0 |
| `--help` | 11 行用法，rc=0 |
| 未知选项 | rc=2 |
| `--config-file` 缺值 | rc=2 |
| `--config-file` 不存在文件 | rc=1 |
| 控制台释放（Start-Process 新控制台） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0**；`AttachConsole(app)`=False |
| ConPTY 注入生效证据 | shell.args 标记文件 `marker=True` |
| 子 shell 退出检测 | 杀子 shell 后 app **1404 ms** 退出 |
| `terminal.shell.args` 规则 | `cmd` → `/K "chcp 65001"`；`powershell.exe` → `-NoLogo -NoExit -Command "chcp 65001"`；`pwsh` → **空**；`args=[]` → `cmd.exe`（无参数）；用户 `args=["/D"]` → `cmd.exe /D` |
| `cargo test --workspace` | **233 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings`（windows-gnu 宿主） | **0 error** |

## C4 键映射（新增单测，`shared:` 提交 4bdfbb7）

`crates/app/src/lib.rs` 追加 `#[cfg(all(test, windows))] mod keymap_tests`（只加测试，`cfg(windows)` 门控，Linux 构建不受影响）：

| 测试 | 覆盖 | 结果 |
|---|---|---|
| `map_logical_key_named_keys` | Enter/Tab/Backspace/Esc/方向键/Home/End/**Insert/Delete/PageUp/PageDown/F1–F12** | pass |
| `map_logical_key_punctuation_and_ascii` | `- = , . ; ' \` \ [ ] /` 空格 + 大小写字母/数字归一 | pass |
| `default_bindings_shift_pageup_pagedown_are_scroll` | 默认表存在 Shift+PageUp→ScrollPageUp、Shift+PageDown→ScrollPageDown，且不带 Ctrl/Alt | pass |

- `cargo test -p leantty-app --lib keymap`：3 passed。
- **ConPTY 键盘转发：未测**（本机无法向 app 注入按键——前台锁/键鼠注入均无法送达，见 3R/3T 记录）。上面只证明"逻辑键 → vt::Key"的映射与默认绑定表，不构成"ConPTY 收到正确字节序列"的证据。

## C5 打包（leantty 0.1.1）

```
scripts/windows/package-release.ps1 -ConptyDir <kit v4>
zip:  vt-verify-win/leantty-0.1.1-windows-x86_64.zip
size: 5 785 895 bytes
sha256: fb2d5b79723b78da87f13b29e8e968b1fdbba196d2513b30854b72d0a93c66a0
```

- 单一顶层目录 `leantty-0.1.1-windows-x86_64/`，10 个条目：`leantty.exe`（12 791 296 / `d86b0d7a…`）、
  `conpty.dll`（119 608 / `e971856f…`）、`OpenConsole.exe`（1 089 336 / `b17e48b0…`）、`leantty.toml`、
  `leantty-portable.cmd`、`LICENSE`、`THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- ConPTY 两文件沿用固定 sha256 + Authenticode（Microsoft Corporation）校验。
- **zip 内文档与仓库逐文件哈希比对（全部一致）**：

| 文件 | zip 内 sha256 | 仓库 sha256 | 一致 |
|---|---|---|---|
| README.md | `771ffe62…` | `771ffe62…` | ✓ |
| README.zh-CN.md | `95754e78…` | `95754e78…` | ✓ |
| CHANGELOG.md | `68903d9f…` | `68903d9f…` | ✓ |
| LICENSE | `45c88805…` | `45c88805…` | ✓ |
| THIRD-PARTY-NOTICES.md | `cd28e4b0…` | `cd28e4b0…` | ✓ |

- 泄漏扫描（exe + zip 内 10 个文件，`strings -n 6`）：仓库路径 `E:\dev`/`dev\tty` **0**、`/leantty/` **0**、
  `\Users\`/`Admin` **0**、主机名 **0**、`ali:8022`/`ssh://ali` **0**、`/root/`、`/home/` **0**；
  `/cargo-home` 569（中性前缀，预期）；盘符路径 3 行（2 处数据碎片 + 1 处依赖内置 Windows 字体目录常量），
  与 5B 结论相同，非身份泄漏。

## C6 收口

- 提交：`4bdfbb7`（keymap 单测，`shared:`）、本报告与 DECISIONS、`package-release.ps1`（版本从 Cargo.toml 读、含 CHANGELOG.md）。
- 轻量 tag：`phase6c-win-done`（名称匹配 `^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$`，打 tag 前 `git status` 干净）。
- 只 push `win` 与该 tag；`git ls-remote` 实际输出见汇报。

## 未解决问题

1. **ConPTY 键盘转发未测**（无输入注入通道）；需要交互式桌面或 debug 钩子才能端到端验证 F1–F12/Insert/Delete/PageUp/PageDown 的字节序列。
2. `crates/render/tests/background_load.rs` 的 5 行调整来自 Linux 侧，本轮未细查其动机（不在 Windows 职责内）。
3. 5B 已知限制仍在：`Start-Process -ArgumentList` 传非 ASCII 路径会破坏参数（harness 限制，非应用缺陷）。

## 手动项（≤3，无法自动覆盖）

1. 解压 `leantty-0.1.1-windows-x86_64.zip` 后双击 `leantty-portable.cmd` 启动，确认无控制台黑框、窗口正常。
2. 在应用内用真实输入法输入中文（含候选窗跟随），确认提交文本正确。
3. 在 `pwsh` 里执行一条输出中文的命令（如 `Write-Output '中文测试'`），确认编码正确。
