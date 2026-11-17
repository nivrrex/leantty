# PHASE3U-WIN：合并 4W / `terminal.shell.args` 接线与实机验证 / 退出回归 / RC2

- 日期：2026-11-17（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`，origin = `ssh://ali:8022/srv/git/tty.git`。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 采集：`APP_PERF_FRAMES=1` + 直接句柄重定向；进程只结束本任务 `Start-Process -PassThru` 启动并核对过命令行的 `app.exe`。

## 1. t1 合并 4W

| 项目 | 结果 |
|---|---|
| `phase4w-done` | `79fbb0ee9ccc22f11a0f1bcb7844cc8250a86b40`；`git cat-file -t` = commit（**轻量 tag，无 `^{}`**，解引用同值） |
| 合并 | `--no-ff`，合并提交 `8694b3c`，0 冲突 |
| 合并带入 | `docs/PHASE4W-LINUX.md`（含 §3 最小补丁文本）、`README.md`、`docs/DECISIONS.md`、`scripts/gates.sh`（去 rustfmt 排除）、4S/4U 报告更正注记 |

## 2. t2 `terminal.shell.args` 接线（唯一被授权的 shared 改动）

4W §3 指出覆盖点在 `crates/pty/src/windows.rs`；本阶段按 3U 例外实施 `shared:` 最小补丁（`ff8da85`）：

| 文件 | 改动 |
|---|---|
| `crates/config/src/lib.rs` | `ShellConfig.args: Vec<String>` → `Option<Vec<String>>`（区分"未配置"与"显式空数组"） |
| `crates/pty/src/lib.rs` | `Pty` trait 新增 `spawn_with_args(..., args: Option<&[String]>, ...)`，**默认实现忽略 args 走既有 `spawn`**（Unix 行为不变） |
| `crates/pty/src/windows.rs` | spawn 拆出 `spawn_inner`；`decide_shell_args(program, configured)`：`Some(a)` 原样返回，`None` 沿用既有默认（cmd.exe → `/K "chcp 65001"`，powershell.exe 保持原 4 项，其余空）；覆盖 `spawn_with_args` |
| `crates/app/src/lib.rs` | spawn 改调 `spawn_with_args(..., shell_args.as_deref(), ...)` |

- 与 4W 提议的一处**有意偏离**（工程判断，置信度高）：4W 表中 `powershell.exe` + 未配置 → `[]`；本实现保留其既有 4 项默认参数
  （`-NoLogo -NoExit -Command "chcp 65001"`），避免既有 UTF-8 行为回归。`pwsh.exe` 不受影响（原本就无默认参数）。
- 单测：`crates/pty/src/windows.rs::tests::decide_shell_args_table` 覆盖 cmd/CMD.EXE/绝对路径 cmd/powershell/pwsh/显式空数组/用户 args。

### 2.1 实机验证（每例取 `app.exe` 子进程命令行原文；标记路径由脚本位置推导）

| 例 | 配置 | 子进程命令行（原文） | 判定 |
|---|---|---|---|
| a | 无 `[terminal.shell]` | `C:\Windows\system32\cmd.exe /K "chcp 65001"` | 与 4W 声明的默认一致（已验证） |
| b | `program="cmd.exe"`，无 args | `C:\Windows\system32\cmd.exe /K "chcp 65001"` | 同 a（已验证） |
| c | `program="cmd.exe"`, `args=["/D","/K","echo X>E:/dev/3u/marker.txt"]` | `C:\Windows\system32\cmd.exe /D /K "echo X>E:/dev/3u/marker.txt"` | **用户 args 生效、无 chcp**；`marker.txt` 生成=True（已验证） |
| d | `program="pwsh"` | `"C:\Program Files\PowerShell\7\pwsh.exe"` | 无 `/K`（已验证） |
| e | `program="cmd.exe"`, `args=[]` | `C:\Windows\system32\cmd.exe` | 显式空数组不再注入默认（已验证） |
| f | 3T 的 shim 绕行 | `grep -rn shim scripts/windows` **0 命中** | 仓库内无引用；`E:\dev\3s\shim` 属仓库外临时工具，未入库 → 标**废弃**（已验证） |

- 结论：**未触发停止条件**（case c 命令行不含 chcp 默认值），说明覆盖点确在 `windows.rs` 且接线正确。
- 原始输出：`vt-verify-win/logs/3u/t2.log`。

## 3. t3 退出检测回归（D = `phase4w-done`，独立 worktree + 独立 `CARGO_TARGET_DIR`）

| 场景 | 组合数 | 结果 | 时延（子 shell 退出/杀 shell → app 退出） |
|---|---|---|---|
| 子 shell 自行退出（cmd `ping -n 4` / pwsh `sleep 4` 后 exit） | 12（sys/bundle × 2 shell × 3 次） | **12/12 关闭** | **1670–2056 ms** |
| 直接结束子 shell（按 PID + 命令行核对） | 12（同上 × 3 轮合并计） | **12/12 关闭** | **936–2113 ms** |

- 均 ≤5 s，符合预期。
- 空闲唤醒（D，cpu 后端，输出停止后 30 s）：**8 条 5 秒统计行**，`wake_timer=5`/5 s → **≈1 次/秒**，符合预期。
- 原始输出：`vt-verify-win/logs/3u/t3-matrix.csv`、`t3-kill.csv`、`t3-idle.log`。

## 4. t4 RC2

```
zip: vt-verify-win/tty-win-x64.zip  bytes=6 607 474  sha256=868ce25430cc12646282de5a15715a4b5d01c31972e150be8615159828149f2f
MANIFEST 首行: "# conpty: OpenConsole.exe FileVersion 1.25.2611.09001"
```

与 RC1 逐文件对比（`diff rc1-manifest rc2-manifest`）**仅 1 行变化**：

| 文件 | RC1 | RC2 |
|---|---|---|
| app.exe | 18 098 452 / `90b58d3f…` | **18 109 900 / `5f95750e…`** |
| config.example.toml | 3 495 / `f4d6862c…` | 同 |
| conpty.dll | 119 608 / `e971856f…` | 同 |
| LICENSE-ConPTY.txt | 1 116 / `5d177f23…` | 同 |
| NOTICE.md | 23 176 / `e7fbaade…` | 同 |
| OpenConsole.exe | 1 089 336 / `b17e48b0…` | 同 |
| README.md | 1 827 / `fa06d773…` | 同 |

- 结论：与预期一致（仅二进制变化；本轮未改打包文档，故 README 不变）。文档变化为仓库文档（PHASE3U-WIN/DECISIONS），不进入 dist。

## 5. 门禁（HEAD = `ff8da85` 之后）

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **217 passed / 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **0 error / 0 warning** |
| `cargo fmt --all --check` | 无输出（全仓 0 偏差） |

## 6. 未解决问题与请求

1. **`ShellConfig.args` 类型变更影响公开结构**（`Vec` → `Option<Vec>`）：Linux 侧配置单测已随之更新（`crates/config` 13 passed），但其它下游若直接构造 `ShellConfig` 需同步。请求：4W/后续阶段确认该签名可长期保留。
2. **`powershell.exe` 未配置时仍注入 4 项默认参数**（与 4W 表不同）：本轮按"不回归"处理；如果 4W 意图是把 powershell 默认也清空，需要再确认（工程判断：清空会让默认 powershell 会话回落到非 UTF-8 代码页）。
3. **t2f shim**：仓库内已无引用；`E:\dev\3s\shim` 为仓库外临时工具（未入库），如需复用应正式落库或彻底删除。

## 7. 手动清单（≤3）

1. 有真实 GPU 的机器：用 RC2 复跑一次 t5（960 宽字形差异）与 `run-tonight.ps1 -Quick`。
2. 用 `program="powershell.exe"` 的配置启动一次，确认中文输出仍为 UTF-8（本轮只测 pwsh 7）。
3. 若下游有直接构造 `ShellConfig` 的代码，确认 `args: Option<Vec<String>>` 的迁移。
