# PHASE8C-WIN：0.2.1 发布包（第一部分）

- 日期：2026-11-18（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 说明：本文件分两部分记录；第一部分（0.2.1 发布包）在本节收口并打 tag，第二部分（吞吐诊断）见文末追加。

## E1 合并

`git ls-remote origin` 实际输出（节选）：

```
b7567b4d102ab8452dd92190ee3ce6a91f36b254	refs/heads/main
e09b015705cf8bc85dc2abdb41f674078c7d0aa7	refs/heads/win
bc5735c61b012a679c1a9491061ac17b8b7fe521	refs/tags/phase8a-done
```

- `phase8a-done` 与任务书 `bc5735c6…` **一致**；`git cat-file -t` = commit（**轻量 tag，无 `^{}` 行**）。
- 合并冲突仅 `docs/DECISIONS.md`（双方文件尾追加）：按"双方保留"解决；合并提交 `71076d8`。
- `crates/` 相对 `phase7d-win-done`（`e09b015`）差异文件清单（全部随合并进入）：

| 文件 | 变化 | 内容 |
|---|---|---|
| `crates/app/src/lib.rs` | +82/−? | 选区交互接线（流式选区） |
| `crates/app/src/selection.rs` | +105 | `stream_row_range` 方向感知行范围 + `stream_tests` 3 例 |
| `crates/render/src/decide.rs` | +29 | 渲染判定相关 |
| `crates/render/tests/incremental.rs` | +19/−? | 增量渲染测试更新 |

## E2 重建与门禁

- `scripts/windows/build.ps1 … --release`：rc=0，`build.ps1 OK`。
- `--version` 原始输出：`leantty 0.2.1`（rc=0）。
- `cargo test --workspace`：**239 passed / 0 failed**。
- `cargo clippy --workspace --all-targets -- -D warnings`（windows-gnu 宿主）：**rc=0、0 error**。

## E3 回归（已验证事实）

| 项 | 结果 |
|---|---|
| `--version` | rc=0，`leantty 0.2.1` |
| `--help` | rc=0 |
| `--config-file` 缺值 / 不存在文件 | rc=2 / rc=1 |
| `-e` 缺程序 | rc=2 |
| `--working-directory "E:\dev\7b\dirs\with space" -e probe` | rc=0，子进程 `cwd=E:\dev\7b\dirs\with space` |
| `--working-directory E:\dev\8c\nodir -e probe` | **rc=1**，stderr `leantty: cannot use working directory 'E:\dev\8c\nodir': … (os error 2)` |
| 控制台释放（`-e cmd.exe /k …`） | 本进程可见 ConsoleWindowClass **1s=0 / 3s=0 / 8s=0** |
| `-e probe`（子进程立刻退出） | leantty **574 ms** 后退出，rc=0，标记文件存在 |
| 默认配置启动（`--config-file config/leantty.windows.toml`） | `stderr_len=0`（无输出）；进程在 20 s 观察窗内仍运行（预期） |

## E4 选区单测（已验证事实，原始测试行）

```
test selection::auto_scroll_tests::auto_scroll_step_direction_and_clamp ... ok
test selection::offset_tests::selection_region_clamps_to_viewport ... ok
test selection::offset_tests::viewport_abs_roundtrip ... ok
test selection::stream_tests::single_row_and_zero_length ... ok
test selection::stream_tests::upward_drag_is_streaming ... ok
test selection::stream_tests::three_row_drag_is_streaming ... ok
```

- 鼠标注入本机不可用 → **拖选手感与真实鼠标点击"未测"**（仅覆盖纯函数/单测层）。

## E5 打包（leantty 0.2.1）

```
zip:  vt-verify-win/leantty-0.2.1-windows-x86_64.zip
size: 5 793 626 bytes
sha256: 25d47aa95f5d215b3e0d6f4bf41581266abeddd489c084c6be7237ba08cd9ec3
```

- 单顶层目录 `leantty-0.2.1-windows-x86_64/`，**10 条目**：`leantty.exe`（12 801 536 / `ddb005fd…`）、
  `conpty.dll`、`OpenConsole.exe`、`leantty.toml`、`leantty-portable.cmd`、`LICENSE`、
  `THIRD-PARTY-NOTICES.md`、`README.md`、`README.zh-CN.md`、`CHANGELOG.md`。
- **zip 内文档 vs 仓库逐文件哈希（raw 与去 CR 均相同，无 CRLF 差异）**：

| 文件 | raw sha256 | 去 CR |
|---|---|---|
| `leantty.toml` ↔ `config/leantty.windows.toml` | `8824111bd1fda252…` | 相同 |
| `README.md` | `9bd64fea263b0892…` | 相同 |
| `README.zh-CN.md` | `80bdcd63b9c992ab…` | 相同 |
| `CHANGELOG.md` | `22bcf202529c6458…` | 相同 |
| `LICENSE` | `45c88805fba0e684…` | 相同 |
| `THIRD-PARTY-NOTICES.md` | `cd28e4b0c6324e26…` | 相同 |

- 泄漏扫描（exe + zip 内 10 文件）：仓库路径 / `\Users\` / 主机名 / `ali:8022` / `/root/` / `/home/` **全 0 命中**。

## 已知限制（第一部分）

1. 鼠标/键盘注入不可用：拖选、点击、IME 均**未测**（只覆盖单测与自动回归）。
2. UNC 可用目录未测（沿用 7B 记录）。
3. pwsh 7.6.6 更新横幅崩溃仍按 6D/6E 结论（与终端尺寸无关）。

## 手动项（≤3，仅无法自动覆盖的）

1. 真实鼠标点击与拖选手感（含单击不留标记、三行流式拖动）。
2. 解压 zip 后用 `leantty-portable.cmd` 启动并做真实输入法测试。
3. 在你机器上对 0.2.1 zip 重做 `ptime tree`，记录用时（用于与第二部分对照）。
