# PHASE3S-WIN：状态核对 / 合并 4T / Windows shell 退出检测实测（停止于 t2）

- 日期：2026-11-16（按系统时间）
- 范围：t0（仓库状态核对与违规提交回滚）、t1（合并 phase4t-done + 构建门禁）、t2（shell 退出检测 A/B/C 对照）。
- **t2 触发任务书停止条件**：构建 C（phase4t-done）在 Windows 上仍然"shell 退出后窗口不关闭"。
  按"记录完数据后停止"的要求，t3~t7 未执行（见 §6），未改 `crates/`、未加依赖、未打 tag。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 0. t0 状态核对

### 0.1 提交清单（`origin/win..win`，核对时 15 提交）

| 提交 | 说明 | 触及 crates/ | 触及非 Windows 专属目录 |
|---|---|---|---|
| bbe7e34 | 4R t1 合并 phase3p-win-done | 否（merge） | 否 |
| 07e5c68 | 4R t2 GPU 空闲跳过回归矩阵 | 否 | `docs/`、`scripts/e2e/` |
| 2525327 | 4R t3 wgpu from_env 审计（4R 任务书授权） | 是（`crates/render/src/gpu/mod.rs`） | 是 |
| 2fd9144 | 4R t4 DECISIONS 更正 | 否 | `docs/` |
| 451a303 | 4R 交付 | 否 | 根目录文档 |
| 9ad5a7c | 4S t1 合并 phase3q-win-done | 否（merge） | 否 |
| 5360252 | 4S t2 空闲定时唤醒治理（4S 任务书授权） | 是（`crates/app/src/lib.rs`） | 是 |
| 63f51e8 | 4S t2 case49 判据更新 | 否 | `scripts/e2e/` |
| 5552a07 | 4S t3 gpu_parity 滚动条对拍（4S 任务书授权） | 是（`crates/render/tests/gpu_parity.rs`） | 是 |
| 1e70825 | 4S t4 README 更正 | 否 | 根目录 README |
| 3306f81 | 4S t5 DECISIONS | 否 | `docs/` |
| f62e456 | 4S 交付 | 否 | 根目录文档 |
| 21ff3de | 合并 phase4s-done | 否（merge） | 否 |
| 9170272 | **3R clipboard.rs 格式化（未授权触及 crates/）** | **是** | 否 |
| 3fe48bc | 3R Windows 交接复测（脚本/日志/报告） | 否 | `vt-verify-win/`、PHASE3R |

- 上述 4R/4S 三个提交对 `crates/` 的改动均由其**各自阶段任务书授权**（提交信息与 PHASE4R/4S 文档可对照），不属违规。
- `9170272` 未获授权：已 `git revert` 生成 **9491fb5**（非 reset/force），回滚后 `crates/app/src/clipboard.rs` 与 `origin/win` 零差异。
- 回滚后普通 `git push origin win`：`1791c76..9491fb5`，`git ls-remote origin refs/heads/win` =
  `9491fb520f3bfbe2acc77a5c0c1c78c589b90e32`（已验证事实）。

### 0.2 上轮回报中可疑内容的取证（grep + `git log -S`）

| 名称 | 结论 | 取证 |
|---|---|---|
| keygen | **与本项目无关/不存在** | `grep -rIl` 0 命中；`git log --all -S keygen` 0 命中 |
| SQLite / sqlite | 同上 | 0 命中 / 0 命中 |
| `send-keys --hex` | 同上 | 0 命中 / 0 命中 |
| `tonight.sh` | 不存在（本项目只有 `run-tonight.ps1` 等 PS 脚本） | `git log -S "tonight.sh"` 0 命中；kit 目录内无 `.sh` |
| `APP_VT` | **本项目无此环境变量**（app 不读取；Windows 的 PTY 恒为 ConPTY） | 仅命中 `vt-verify-win/gpu_probe.ps1`、`idle_ctl.ps1`、`idle_test.ps1`（3R 我自建脚本）；`crates/` 0 命中 |
| `process_group` | 存在但含义不同：ConPTY 测试里用 `GenerateConsoleCtrlEvent(pid)` 的进程组参数，不是"孤儿进程修复" | `crates/pty/tests/pty_windows.rs`；`git log -S` 命中 62f5db2、0446484 |
| `watchdog` | 仅 vendor 导入提交里出现（`git log -S` 命中 0a1e58d），`crates/` 无本项目实现 | `git log -S watchdog` |
| `F7a` / `F7b` | **不存在**（无该编号任务、无相关代码） | `grep -rIl` 0 命中；提交信息 0 命中 |
| `E:\dev\tty-4q` | 存在：detached HEAD `1e10fbc`，与 `win` 的工作树**零差异**（`git diff --name-only win...HEAD` 为空），未被本任务使用，也未合并 | 实测 |
| "clipboard.rs 用 arboard 实现剪贴板抽象" | 真实存在，但**不是 3R 引入**：`crates/app/src/clipboard.rs` 早于 3R 就在仓库内（arboard 依赖见 `Cargo.toml`/`Cargo.lock`）；3R 只做了格式化（已回滚） | `git log -S arboard`、`crates/app/Cargo.toml` |

结论：上轮回报中 keygen/SQLite/send-keys/tonight.sh/APP_VT/F7a/F7b 均属**不存在或与本仓库无关**的内容
（`APP_VT` 是我自建验证脚本里的无效旋钮，app 从未读取）；仓库未见混入其他项目文件，未触发 t0d 停止条件。

## 1. t1 合并与门禁

| 项目 | 结果（已验证事实） |
|---|---|
| `phase4t-done` | 本地与 `ls-remote` 均为 `8132ade1984084f8f95186529293d4d2f31bf06c`；`git cat-file -t` = commit（**轻量标签，无 `^{}`**） |
| 合并 | `git merge --no-ff 8132ade`，**0 冲突**，合并提交 `0f7…`（见 `git log`） |
| 合并带入 | `crates/app/src/lib.rs`（4T t3 Windows 存活检查）、`docs/DECISIONS.md`、`PHASE4T-LINUX.md`、`scripts/e2e/timer_dep_matrix.sh` |
| Windows 专属文件整体 sha256 | 合并前后均 `8540f79730490feab971ddcbfd3616e7cb8ccb43c8a462801df5801d1dbde1fe`（范围：`crates/pty/src/windows.rs` + `crates/pty/tests/pty_windows.rs` + `scripts/windows/**`） |
| debug / release 构建 | 均 rc=0，**0 warning** |
| clippy（dev / release，`-D warnings`） | **0 / 0** |
| `cargo test --workspace` | **211 passed / 0 failed**（`logs/3s/test-workspace2.log`） |
| 首次测试失败说明 | 未建 `C:\dev\tty` 前，`pty_windows` 的 `t6_probe_spawn_from_devtty_cwd` 因**测试内硬编码 `C:\dev\tty`**（仓库已移到 E:）失败 1 例；以 `mklink /J C:\dev\tty E:\dev\tty` 复原原环境后重跑全绿。测试硬编码属共用代码，未改，已列入请求栏 |
| wgpu 版本 | `Cargo.lock`：`wgpu 30.0.2`（精确一致） |
| gpu_parity + `APP_GPU_ALLOW_SOFTWARE=1` | **10 passed / 0 failed**（DX12 WARP，42.1s）；无该变量时 GPU 用例 SKIP 并打印原因 |

## 2. t2 shell 退出检测（停止点）

### 2.1 方法与本机约束

- 三个 release 构建：A=`phase4r-done`(451a303)、B=`phase4s-done`(f62e456)、C=`phase4t-done`(8132ade)，
  各用独立 worktree（`E:\dev\3s\wt-*`）与独立 `CARGO_TARGET_DIR`（`E:\dev\3s\target-*`），均在仓库外。
- **本机限制**：`config.terminal.shell.args` 已解析但**未接线到 PTY spawn**（`crates/config` + DECISIONS 记录，
  本任务不得改 `crates/`），GUI 输入注入在本机不可用（前台锁；`keybd_event`/`mouse_event`/`PostMessage`
  均未送达，见 3R 记录）。因此用无依赖 `shim.exe` 作配置里的 shell：它执行 `cmd /c <cmd>` 或
  `pwsh -NoProfile -c <cmd>` 后按子进程退出码退出，即 app 可见的 shell 进程会**自行退出**。
  **注入生效证据**：每条命令先 `echo ok>marker.txt`，脚本以 marker 文件出现计时（实测 925–2505 ms）。
- 矩阵：3 构建 × 2 ConPTY（`sys`=系统 conhost、`bundle`=随附 conpty.dll+OpenConsole.exe）× 2 shell
  （cmd/pwsh；cmd 为 `ping -n 4` 约 3s 后 exit，pwsh 为 `Start-Sleep 4` 后 exit）× 3 次；
  判定 = 子进程退出后 15 s 内 app 进程是否退出（`WaitForExit(15000)`）。

### 2.2 结果（已验证事实，`E:\dev\3s\out\t2-matrix.csv`）

| 构建 | shell | marker_ms | 子进程退出→app 退出 (ms，3 次) | 15 s 内退出 | 未退出时 CPU (s) |
|---|---|---|---|---|---|
| A=4r | cmd | 927–1471 | 3158 / 3165 / 3136（sys）、3124 / 3095 / 3151（bundle） | **是（12/12）** | — |
| A=4r | pwsh | 1780–1994 | 4310 / 4337 / 4306、4226 / 4299 / 4300 | **是（12/12）** | — |
| B=4s | cmd | 990–1714 | -1（六次均未退出） | **否（0/6）** | 0.938–1.047 |
| B=4s | pwsh | 1569–1982 | -1 | **否（0/6）** | 0.953–1.078 |
| C=4t | cmd | 925–1141 | -1 | **否（0/6）** | 0.969–1.078 |
| C=4t | pwsh | 1567–2505 | -1 | **否（0/6）** | 0.938–1.188 |

（`sys` 与 `bundle` 两列结果一致，合并展示；完整逐行数据见 csv。）

另测"直接结束 shell 子进程"（只杀本测试自己启动的 `shim.exe`，按命令行唯一路径匹配）：

| 构建 | 结果（4 组合） |
|---|---|
| A | **退出，107–127 ms** |
| B | 15 s 内**未退出**（4/4） |
| C | 15 s 内**未退出**（4/4） |

### 2.3 判定与根因取证

- **B 重现"输入/退出后窗口不关闭"**（A 正常）→ 4T 对 Linux 的判断（4S 的空闲静默引入回归）在 Windows 上**成立**。
- **C 仍然不关闭** → 触发任务书停止条件，未改代码。C 的窗口在子进程退出后 15 s 内保持存活，
  标题 `app.exe`，CPU 约 1.0 s（≈启动），截图 2963 B（空白终端 + 光标）。
- 进一步取证（构建 C，release，`APP_PERF_FRAMES=1`）：
  1. 子 shell 存活 30 s、无输出时，13 s 内**只有 1 条 5 秒统计行**（`loops=1`）→ **1 秒存活唤醒从未发生**；
  2. 杀死 shell 后 4 s 内 app 不退出；随后对窗口做一次 `ShowWindow(SW_MINIMIZE/RESTORE)`（不涉及输入注入），
     app **立即退出** → `try_wait` 路径有效，缺的只是"定时唤醒"。
- 结论（工程判断，置信度高）：4T 的 `#[cfg(target_os="windows")]` 存活唤醒块在 C 上**未生效**；
  当前证据指向"该块在空闲那次 `about_to_wait` 中未被到达/未留下 `WaitUntil`"（否则 `loops` 会以 1/s 增长）。
- 待验证假设（需在 crates 内加 debug 钩子后确认，本任务不允许）：候选为 `about_to_wait` 在渲染段
  提前 `return`（surface/快照错误路径）导致尾部唤醒块被跳过；另一候选是 `wait_scheduled`
  在该次通过后仍为 `true`。推翻条件：加一行计数/日志即可区分。

## 3. 未做项（停止条件触发，均未执行）

| 任务 | 状态 | 原因 |
|---|---|---|
| t3 拖选自动滚动 / OSC 标题 | **未做** | t2 停止条件；且本机 GUI 输入注入不可用（前台锁），拖选无法构造 |
| t4 空闲唤醒 B/C 对照 | 部分数据 | C 的"无 1 s 唤醒"已顺带测得（§2.3）；正式 B/C 对照未做 |
| t5 截图差异分布 | 未做 | 停止条件 |
| t6 硬编码路径修复 | 未做 | 停止条件（已定位：`scripts/windows/e2e/wb2_lib.ps1` 的 `$ROOT='C:\dev\tty'`） |
| t7 候选部署包 RC1 | 未做 | 停止条件 |

## 4. 请求栏（需 Linux 侧/授权处理，本任务不改 crates/）

1. **C 的 Windows 存活唤醒未生效**：需在 `crates/app/src/lib.rs` 的 `about_to_wait` 尾部加计数/日志
   定位"块未到达"还是"`wait_scheduled` 残留"，修复后按本报告 §2.2 同一矩阵复测。
2. **测试内硬编码路径**：`crates/pty/tests/pty_windows.rs::t6_probe_spawn_from_devtty_cwd` 写死 `C:\dev\tty`，
   仓库迁移后必然失败；建议改为 `env!("CARGO_MANIFEST_DIR")` 推导。
3. **`terminal.shell.args` 未接线**：配置可写 args 但不生效，导致任何"指定命令/参数"的场景都只能靠
   自建 shim 绕过；建议在 4T 后续阶段接线或明确文档标注。
4. **`scripts/windows/e2e/wb2_lib.ps1` 的 `$ROOT='C:\dev\tty'`**（t6 已定位，未改）。

## 5. 手动清单（≤3）

1. 在 Linux 侧按 §4.1 定位并修复存活唤醒，然后重跑本报告 §2.2 矩阵（脚本 `E:\dev\3s\t2matrix.ps1`）。
2. 在有真实 GPU 的 Windows 机器上补 `gpu_parity` 与 §3 的 t4/t5（本轮只能用 WARP）。
3. 交互式桌面下补 t3（拖选自动滚动、OSC 标题），本机前台锁无法注入。

## 6. 交付与回滚

- 本轮提交：仅文档/脚本（`PHASE3S-WIN.md`、`docs/WIN-NOTES.md`、`vt-verify-win/logs/3s/**` 中的门禁日志），
  **未改 `crates/`、未加依赖、二进制未入库**；**未打 tag**（任务书在 t2 停止，t8 未完成）。
- 回滚：`git revert 9170272` → 9491fb5（已 push，`ls-remote` 见 §0.1）。
