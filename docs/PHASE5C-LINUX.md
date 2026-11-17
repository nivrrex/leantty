# PHASE5C-LINUX：合并 5B / 门禁补强 / 发布一致性 / deb / Windows zip 核对 / 导出

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 5B | 轻量 tag `59bdf0f`；`crates/` 仅 `app/src/lib.rs` 两处（doc 缩进 + 容量 2 缓冲）；Cargo/vendor 零变更；合并后断言为空；gates 全过 → `phase5c-t1`（`8d4d4de`）|
| t2 门禁补强 | gates 增至 **10 步**，新增 `cargo clippy --target x86_64-pc-windows-gnu -- -D warnings`（0 告警）|
| t3 README 核对 | 与 5B 实测逐条相符，无矛盾；按 t3b 追加 1 条 Linux 专属故障排查 → 用户裁决不重打 zip → `phase5c-t2` |
| t5 zip 核对 | 大小/sha256 与自述一致；9 条目；4 个必需文件哈希一致，2 个 README 按裁决不一致（差异恰为新增条目）|
| t4 deb / t6 导出 / t7 收尾 | 见后文 |

## 1. t1 合并与审计（已验证事实）

```
$ git ls-remote ali | grep -E "phase5b|refs/heads/(main|win)$"
0b7115ef483290e7802930fca35673936c4d2639	refs/heads/main
59bdf0f0488a839e30e7020d7b8b902336ffdcfd	refs/heads/win
59bdf0f0488a839e30e7020d7b8b902336ffdcfd	refs/tags/phase5b-win-done      ← 轻量 tag（无 ^{} 行）

$ git diff --stat phase5a-done phase5b-win-done -- crates/ Cargo.toml Cargo.lock vendor/
 crates/app/src/lib.rs | 11 +++++++----
 1 file changed, 7 insertions(+), 4 deletions(-)
```

完整 diff 恰为两类：

1. `free_if_sole_user` 的 SAFETY 文档缩进与中文引号（`"本进程没有控制台"` → `“…”`）；
2. 控制台释放从 `GetConsoleProcessList(null_mut(), 0)` 改为**容量 2 的缓冲**
   （5B 探针实测：`null, 0` 返回 0 且 `GetLastError=87`，旧实现永远不会释放）。

`Cargo.toml` / `Cargo.lock` / `vendor/` 零变更。`git merge --no-ff` 无冲突（`6973c3b`），
合并后 `git diff --stat phase5b-win-done HEAD -- crates/ Cargo.toml Cargo.lock vendor/` 为**空**。

## 2. t2 门禁补强（已验证事实）

`scripts/gates.sh` 现为 10 步；新增第 9 步：

```sh
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

补齐"cfg(windows) 代码在 Linux 上不参与 debug/release clippy"的缺口。该步在 Linux 上可直接
运行（无需缩减到单个 crate），实测 **0 告警**（含 5B 的控制台释放代码）。第 5 步的
"e2e 必须有汇总行"断言保持有效。

## 3. t3 README 与 5B 实测核对

### 3.1 逐条核对（只读，已验证事实）

| README 陈述 | 5B 实测依据 | 结论 |
|---|---|---|
| 随附 ConPTY 两文件必须同目录，缺一个回落系统控制台并警告 | `PHASE5B-WIN.md` §13（两文件 sha256 + Authenticode）| 相符 |
| 未签名 → SmartScreen 首次启动告警 | 5B 发布说明与实测 | 相符 |
| 双击/`.cmd` 启动后应用不持有自己的控制台窗口 | 5B §t4d：1s/3s/8s 控制台窗口数均为 0，`AttachConsole(app)`=False | 相符（README 未做此陈述，故无冲突）|
| 命令行共享控制台时控制台保留、输出可重定向 | 5B §t4d b/c：`2>file` 正常、批处理继续执行 | 相符 |
| 用模板 `--config-file` 启动 stderr 为空 | 5B §13：**stderr 0 字节** | 相符（README 未承诺"无输出"，非绝对表述）|
| 只写已测系统版本 | README 未声明任何 Windows 版本号 | 相符 |

未发现与实测冲突的陈述。

### 3.2 追加的故障排查条目（t3b → t3c'）

`request_inner_size` 提示的全文与触发条件（已验证事实）：本机 Xvfb 报 100 dpi
（`xdpyinfo`: `resolution: 100x100 dots per inch`），winit 的 scale ≠ 1.0 → 窗口创建后按真实
scale 重算格子并 `request_inner_size` 修正尺寸；winit 0.30 返回 `None` 表示"已异步提交、结果
稍后经 `Resized` 返回"。消息全文：

```
request_inner_size: 尺寸请求异步提交，实际尺寸以 Resized 事件为准
```

确属信息性提示 → 两份 README 各追加一条（编号接在现有第 5 条之后），结构保持一致。

### 3.3 一致性证据（t3d）

```
$ git diff --stat
 README.md       | 1 +
 README.zh-CN.md | 1 +
 2 files changed, 2 insertions(+)
```

逐文件 sha256（zip 内 vs 仓库）：

| 文件 | zip | 仓库 | 结论 |
|---|---|---|---|
| LICENSE | `45c88805fba0e684` | `45c88805fba0e684` | 一致 |
| THIRD-PARTY-NOTICES.md | `cd28e4b0c6324e26` | `cd28e4b0c6324e26` | 一致 |
| leantty.toml（config/leantty.windows.toml）| `623fc939f475d59b` | `623fc939f475d59b` | 一致 |
| leantty-portable.cmd | `4edfa1ababd38fcd` | `4edfa1ababd38fcd` | 一致 |
| README.md | `9b029ffd063c29e6` | `9ee36c2bbe1c5de6` | 不一致（预期）|
| README.zh-CN.md | `a234dde0da591efc` | `cda0862344bc259c` | 不一致（预期）|

`diff -u` 证明两份 README 的差异**各为 +1 行、−0 行**（就是新增的第 6 条），即 zip 内版本 =
仓库版本去掉该条。**zip 内 README 比仓库少一条 Linux 专属故障排查**，源码与二进制一致，
用户裁决不重打 zip。
