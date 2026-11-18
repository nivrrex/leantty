# PHASE7D-WIN：合并 7C（hinting 默认 false）/ 0.2.0 重建 / hinting 验证 / 回归 / 打包

- 日期：2026-11-18（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## D1 合并

`git ls-remote origin` 实际输出（节选）：

```
1bd78510c2c9da12d28b8f1f4d80dda0afb139b2	refs/heads/main
b9c4f200c7f18a385e59fcd463acb745f2575963	refs/heads/win
1bd78510c2c9da12d28b8f1f4d80dda0afb139b2	refs/tags/phase7c-done
```

- `phase7c-done` 与任务书 `1bd78510…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- `--no-ff` 合并，**0 冲突**（含 `docs/DECISIONS.md`，本次未产生冲突段），合并提交 `6dbf583`。
- `crates/` 相对 `phase7b-win-done`（`b9c4f20`）差异——与预期一致，共 **3 行**：

| 文件 | 行 | 变化 |
|---|---|---|
| `crates/config/src/lib.rs` | 1 | `hinting: true` → `hinting: false` |
| `crates/render/src/lib.rs` | 1（默认值） | `hinting: true` → `hinting: false` |
| `crates/render/src/lib.rs` | 1（文档注释） | “new() 时打印一次 unsupported 日志” → “默认 false，仅当用户显式设为 true 时打印一次提示” |

另有 `config/leantty.windows.toml` 的一行注释更新（“keep it false to avoid a startup warning” → “setting this to true only prints a notice”），属示例配置，非 crates。

## D2 重建与门禁

- `scripts/windows/build.ps1 … --release`：rc=0，`build.ps1 OK`。
- `--version` 原始输出：`leantty 0.2.0`（rc=0）。
- `cargo test --workspace`：**236 passed / 0 failed**。
- `cargo clippy --workspace --all-targets -- -D warnings`（windows-gnu 宿主）：**rc=0、0 error**。

## D3 hinting 验证（已验证事实，原始输出）

采集方式：`ProcessStartInfo.RedirectStandardError`（直接句柄）+ 运行 4 s 后按**记录 PID 且核对命令行**结束进程。

| 配置 | 实测 |
|---|---|
| 不写 `hinting`（`nohint.toml`，仅注释提及） | `pid=14176 alive_before_kill=True cmdline_matches=True stderr_len=0`，`stderr_raw` 为空 |
| 显式 `hinting = true` | `pid=13452 alive_before_kill=True cmdline_matches=True stderr_len=54`，stderr 原文：`hinting: fontdue 不支持 hinting，已解析未生效（unsupported）`（控制台显示为 GBK 乱码，文件内容为 UTF-8） |

→ 默认 `false` 时**不再打印**该提示；显式 `true` 时仍打印，与 7C 的设计一致。

## D4 回归（已验证事实）

| 项 | 结果 |
|---|---|
| `--version` | rc=0，`leantty 0.2.0` |
| `--help` | rc=0 |
| `--config-file` 缺值 / 不存在文件 | rc=2 / rc=1 |
| `-e` 缺程序 | rc=2 |
| `--working-directory "E:\dev\7b\dirs\with space" -e probe` | rc=0，子进程 `cwd=E:\dev\7b\dirs\with space` |
| `--working-directory E:\dev\7d\nodir -e probe` | **rc=1**，stderr `leantty: cannot use working directory 'E:\dev\7d\nodir': … (os error 2)` |
| 控制台释放（`-e cmd.exe /k …` 常驻） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0** |
| `-e probe`（子进程立刻退出） | leantty **615 ms** 后退出，rc=0，标记文件存在 |

## D5 打包（leantty 0.2.0）

```
zip:  vt-verify-win/leantty-0.2.0-windows-x86_64.zip
size: 5 791 602 bytes
sha256: a60b16433d561c787abaea6306732cebf2920f9b31613de92b1c870ef5a3a965
```

- 单顶层目录 `leantty-0.2.0-windows-x86_64/`，**10 条目**：`leantty.exe`（12 797 952 / `f5a5ef41…`）、
  `conpty.dll`、`OpenConsole.exe`、`leantty.toml`、`leantty-portable.cmd`、`LICENSE`、
  `THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- **zip 内文档 vs 合并后仓库逐文件哈希（raw 与去 CR 后均相同，无 CRLF 差异）**：

| 文件（zip ↔ 仓库） | raw sha256 | 去 CR 后 |
|---|---|---|
| `leantty.toml` ↔ `config/leantty.windows.toml` | `8824111bd1fda252…` | 相同 |
| `README.md` | `a3eac279f1e5a76d…` | 相同 |
| `README.zh-CN.md` | `1c9ea674806355ea…` | 相同 |
| `CHANGELOG.md` | `e20c6113b6548458…` | 相同 |
| `LICENSE` | `45c88805fba0e684…` | 相同 |
| `THIRD-PARTY-NOTICES.md` | `cd28e4b0c6324e26…` | 相同 |

- 泄漏扫描（exe + zip 内 10 文件）：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root/` / `/home/` **全 0 命中**。

## 已知限制

1. 键盘/输入注入仍不可用：IME、`-e` 交互程序的键盘输入均**未测**。
2. UNC 可用目录行为仍未测（7B 记录：本机 `\\localhost\C$` 不可访问）。
3. `crates/pty/tests/pty_windows.rs` 的 `spawn_child_inherits_process_cwd` 会临时 `set_current_dir`，用进程内互斥串行化；与其它改动 cwd 的测试并发时仍依赖测试框架的调度（未观察到失败）。

## 手动项（≤3，仅无法自动覆盖的）

1. 解压 zip 后用 `leantty-portable.cmd` 启动，并做一次真实输入法测试。
2. 在 pwsh 中执行 `Write-Output '中文测试'`。
3. 用 `-e pwsh` 运行交互程序，确认键盘输入正常。
