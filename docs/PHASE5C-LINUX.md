# PHASE5C-LINUX：合并 5B / 门禁补强 / 发布一致性 / deb / Windows zip 核对 / 导出

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 5B | 轻量 tag `59bdf0f`；`crates/` 仅 `app/src/lib.rs` 两处（doc 缩进 + 容量 2 缓冲）；Cargo/vendor 零变更；合并后断言为空；gates 全过 → `phase5c-t1`（`8d4d4de`）|
| t2 门禁补强 | gates 增至 **10 步**，新增 `cargo clippy --target x86_64-pc-windows-gnu -- -D warnings`（0 告警）|
| t3 README 核对 | 与 5B 实测逐条相符，无矛盾；按 t3b 追加 1 条 Linux 专属故障排查 → 用户裁决不重打 zip → `phase5c-t2` |
| t5 zip 核对 | 大小/sha256 与自述一致；9 条目；4 个必需文件哈希一致，2 个 README 按裁决不一致（差异恰为新增条目）|
| s1~s7 抹除痕迹 | 4 处注释/文档 + 1 行测试断言消息；构建三次同 sha256 `4bf11fbf…` |
| s8 仅报告模式 | 命中 2 条（均在导出文件内），只列不改 |
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

## 4. s1~s7 抹除个人环境痕迹

### 4.1 逐处原文与判定（全部为注释/文档，无可执行代码）

| 位置 | 原文片段 | 性质 | 改后 |
|---|---|---|---|
| `scripts/windows/conpty/fetch-conpty.ps1:14-15` | `-OutDir C:\some\dir` / `-Proxy http://127.0.0.1:7777` | `#` 用法注释 | `<path>` / `<proxy-url>` |
| `scripts/windows/conpty/README.md:36` | `-Proxy http://127.0.0.1:7777` | 文档 | `-Proxy <proxy-url>` |
| `scripts/windows/package.ps1:22` | `-OutDir D:\dist\tty -Proxy http://127.0.0.1:7777` | `#` 用法注释 | `<output-dir>` / `<proxy-url>` |
| `crates/pty/src/windows.rs:188` | `（如 C:\dev\tty\cmd.exe）` | `///` 文档注释 | `（如 <dev-root>\cmd.exe）` |
| `crates/render/tests/background_load.rs:78` | `（home-pc 实测 Noto CJK）` | 断言消息字符串 | `（如 Noto CJK）` |

`crates/pty/src/windows.rs` 行数 389 → 389，diff 为 1+/1−；`background_load.rs` 112 行不变，1+/1−。

### 4.2 二进制不受影响 + 构建可复现

```
phase5c-t2 干净 worktree，全量构建 #1：4bf11fbf1537961cd4c8be980d1b62683bd87e44382cbdfbe9175b251b46e413 (12,000,968 B)
phase5c-t2 干净 worktree，rm -rf target 后 #2：同上（可复现）
含 scrub 修改的主工作树，cargo clean 后 #3：同上
含 s7 修改的主工作树，cargo clean 后 #4：同上
```

### 4.3 扫描结果（导出目标范围）

`A=0  B=3  C=0  豁免=12  仅报告=2`

- B 类 3 条：`crates/pty/src/windows.rs` 的 `C:\Windows\System32` / `C:\Windows\system32\conhost.exe`（系统常量）。
- 豁免 12 条：见 §4.4。
- 仅报告 2 条（**都位于会被导出的文件**，按 s8 标红、未修改、交用户裁定）：
  - `crates/app/src/lib.rs:120`：`// 阶段 5B t4b（已验证事实）：本机 Win10 LTSC 上 ...`
  - `scripts/windows/conpty/README.md:7`：`Windows 10 LTSC 19044 hosts conhost's ConPTY, ...`

### 4.4 豁免清单（scripts/release/scan-allow.txt）

| 文件 | 命中 | 原因 |
|---|---|---|
| `crates/app/src/cli.rs` | `/home/u/…` | 单元测试里构造的合成配置路径 |
| `crates/pty/tests/pty_windows.rs` | `127.0.0.1` | 仅作为 ping 的目标地址 |
| `crates/pty/src/windows.rs` | `C:\Windows` | Windows 系统常量（conhost/cmd 默认目录） |
| `vendor/*` | `/home/user/`、`127.0.0.1` | 上游 libghostty-rs 文档示例 |

## 5. s9 重新执行结果（已验证事实）

### 5.1 门禁与 tag

`scripts/gates.sh` **10 步全过**（HEAD 于 `phase5c-t3`），e2e **115/115**（GPU 轮 10/10）；
`phase5c-t3` = `3504fb2a8f12f8f6f2f1ab07861b50dc4a9dcb6b`。

### 5.2 deb（重建）

- 大小 **3,514,444 B**，sha256 `9f200ec1f5e20fa5fa90a132c58042abe1077cbd1b416bb7eaf9338c9a1f8d3c`。
- 与上一版 `2fa3c387…` 不同属**预期**：包内 `README.md`/`README.zh-CN.md` 多了 Linux 故障排查条目，
  且新增了源码头里的一处测试断言消息改动（不进二进制）——打包内容变化必然改包哈希。
- 包内 `/usr/bin/leantty` 的 sha256 = **`4bf11fbf1537961cd4c8be980d1b62683bd87e44382cbdfbe9175b251b46e413`**，
  与 s3/s7c 的可复现构建值一致。
- Depends 未变：`libc6, libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6,
  libxkbcommon0, libxkbcommon-x11-0`；Recommends `libvulkan1`。

### 5.3 导出与扫描

导出目录 167 个文件（3.9 MB）：`A=0  B=3  C=0  豁免=15  仅报告=2`。
- B 类 3 条：`crates/pty/src/windows.rs` 的 `C:\Windows\System32` 等系统常量。
- 豁免 15 条：见 §4.4（单元测试合成路径、ping 目标回环、Windows 系统常量、vendor 上游文档）。
- 仅报告 2 条（**都在导出文件内，未修改**）：`crates/app/src/lib.rs:120`（"本机 Win10 LTSC"）、
  `scripts/windows/conpty/README.md:7`（"Windows 10 LTSC 19044"）。
- deb 与 zip（容器文件本身）：`A=0 B=0 C=0`。
- zip 解包目录：`A=0  B=1  C=0  豁免=121  仅报告=0`；B=1 是 `leantty.exe` 里的 `C:\Windows\Fonts`
  系统常量，121 条豁免全部来自随附的 Microsoft ConPTY 二进制（其内部 `C:\__w\...` 构建机路径）。

### 5.4 干净检出验证（t6e）

复制导出目录到临时目录（路径含空格），按 `docs/BUILDING.md` 注入 `ZIG`/`GHOSTTY_SOURCE_DIR`/
`GHOSTTY_ZIG_SYSTEM_DIR` 后：`cargo build --release --locked` 成功（4m01s），
`cargo test --workspace` **全部通过，0 失败**（无因删开发文件而失败的用例）。

### 5.5 发布资产（t6f）

`leantty-release-assets/`：
- `leantty_0.1.0_amd64.deb`（3,514,444 B，sha256 `9f200ec1…`）
- `leantty-0.1.0-windows-x86_64.zip`（5,783,314 B，sha256 **`4c33a4593dcb66b532423d4c0b215fa26789ca346d42d1c05dc82afab9de0acb`**，与 Windows 侧自述一致）
- `SHA256SUMS`（仅这两个文件名，不含路径）

### 5.6 一致性结论（t6g）

- **zip 内 `README.md` 与 `README.zh-CN.md` 比仓库各少一条 Linux 专属故障排查**（用户裁决不重打 zip）；
  其余 4 个文件（LICENSE、THIRD-PARTY-NOTICES.md、leantty.toml、leantty-portable.cmd）与仓库逐字节一致。
- **crates/ 相对 Windows 构建源码的差异只有两处**：`crates/pty/src/windows.rs` 一行文档注释
  （行数不变 389 → 389）、`crates/render/tests/background_load.rs` 一行测试断言消息；
  二者**都不进入 release 二进制**，Linux release 二进制 sha256 经 s3（两次干净构建）与 s7c（清空 target
  重建）证明**未变化**（`4bf11fbf…`）。因此 zip 对应的源码与 zip 内二进制在功能上与当前源码一致。

## 6. 交付物摘要

| 项 | 值 |
|---|---|
| 三个上传文件 | `leantty_0.1.0_amd64.deb`、`leantty-0.1.0-windows-x86_64.zip`、`SHA256SUMS` |
| 建议仓库描述 | `A lightweight terminal emulator with CPU and optional wgpu/Vulkan rendering.` |
