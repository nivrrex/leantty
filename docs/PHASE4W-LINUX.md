# PHASE4W-LINUX：合并 3T + `terminal.shell.args` 修正定位 + 门禁/文档对齐

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 | `phase3t-win-done` 为**轻量 tag**；`crates/` 差异仅两处预期文件；`--no-ff` 合并无冲突；gates 全过 → tag `phase4w-t1` |
| t2 `shell.args` | **覆盖点在 Windows 专属文件**（`crates/pty/src/windows.rs`），按 t2a 不改；另查明 `Pty::spawn` **没有 args 参数**，接线需动 trait（与"Pty trait 签名不得改"冲突）→ 本项停止，给出精确位置与最小补丁文本（§3） |
| t3 门禁对齐 | 两个 Windows pty 文件已由 3T 格式化，全仓 `cargo fmt --check` **0 偏差** → 去掉排除名单（自检红/绿均验） |
| t4 文档 | README 更正 4S §3 归因 + 新增 `terminal.shell` 规则节；DECISIONS 更正条目（删除线+追加）；4S/4U 报告加更正注记 |
| t5 收尾 | gates 全过、e2e 全过、tag `phase4w-done`、push main + tag |

## 1. t1 合并与范围核查

### 1.1 `phase3t-win-done` 是轻量 tag（已验证事实）

```
$ git ls-remote ali | grep -E "refs/heads/(main|win)$|phase3t|phase4v-done"
d71879b8499cd902cb74168c09667d016e1c9884	refs/heads/main
65e3dfdf90a29e5acab5521decac36798835b220	refs/heads/win
65e3dfdf90a29e5acab5521decac36798835b220	refs/tags/phase3t-win-done
d71879b8499cd902cb74168c09667d016e1c9884	refs/tags/phase4v-done
```

`refs/tags/phase3t-win-done` 与 `refs/heads/win` 指向同一 hash 且**没有 `^{}` 行** → 轻量 tag。

### 1.2 合并前的 `crates/` 范围核查（已验证事实）

```
$ git diff --stat phase4v-done phase3t-win-done -- crates/
 crates/pty/src/windows.rs       | 20 ++++++++------
 crates/pty/tests/pty_windows.rs | 59 ++++++++++++++++++++++++++++++++---------
 2 files changed, 58 insertions(+), 21 deletions(-)
```

与预期完全一致（仅两个 Windows 专属文件），无其它 `crates/` 变更 → 继续合并。

### 1.3 合并

```
$ git merge --no-ff phase3t-win-done^{commit} -m "merge: phase3t-win-done"
（无冲突，自动合并；新增 vt-verify-win/logs/* 等 Windows 采集日志）
$ git diff --stat HEAD phase3t-win-done -- crates/
（空 → 合并后 crates/ 与 3T 完全一致）
```

合并提交 `1161f2f`；gates 9 步全过（e2e 110/110、GPU 轮 10/10）→ `phase4w-t1`。

并入的 Windows 侧提交：`806f053`（pty 两文件 rustfmt，`shared:`）、`51aa45c`（pty 测试去掉
`C:\dev\tty` 硬编码，改用 `env!(CARGO_MANIFEST_DIR)` 推导，`shared:`）、`c34d7f6`（`wb2_lib.ps1`
用 `$PSScriptRoot` 推导根目录）、`dea3ebd`（RC1 打包 + 日志）、`65e3dfd`（`PHASE3T-WIN.md` 交付）。

## 2. t2 `terminal.shell.args`：覆盖点定位与结论

### 2.1 覆盖点（已验证事实，全部位于 Windows 专属文件，本阶段不得修改）

- `crates/pty/src/windows.rs:97-107` `WindowsPty::utf8_codepage_args(shell)`：
  `cmd.exe` → `["/K", "chcp 65001"]`；`powershell.exe` → `["-NoLogo","-NoExit","-Command","chcp 65001"]`；
  其他 → 空。**按 shell 名无条件生成，不看用户配置**。
- `crates/pty/src/windows.rs:221-225`（`spawn` 内）：`CommandBuilder::new(program)` 后只追加
  `Self::utf8_codepage_args(shell)`，**没有任何 args 入参**。

### 2.2 更上游的事实：args 根本没接到 PTY 层

- `Pty` trait 的 `spawn(&mut self, shell: &str, cols: u16, rows: u16)`（`crates/pty/src/lib.rs`）
  **没有 args 参数**；app 侧 `crates/app/src/lib.rs:1212` 只传 `&config.shell`。
- `crates/config/src/lib.rs:192-196` 已解析 `terminal.shell.args`，注释写明"PTY spawn 接口当前
  不含 args（DECISIONS 登记），接线待后续阶段"；且 `ShellConfig.args: Vec<String>` 把
  **"未配置"与"显式空数组"折叠成同一个空 Vec**，无法区分二者。

### 2.3 结论（本项按 t2a 停止，不做修改）

要让"用户显式配置的 args 优先"生效，需要三步同时改，其中两步要么在 Windows 专属文件、
要么触碰 trait 签名：

1. **配置**（shared，可改）：`ShellConfig.args: Option<Vec<String>>`——保留 `None` 表示未配置、
   `Some(vec![])` 表示显式空数组。**不改变 Linux 既有 TOML 语义**（Linux 上该字段本就不生效）。
2. **trait**（shared，但**本阶段规则禁止改 Pty trait 签名**）：见 §3 的最小补丁（加带默认实现的
   `spawn_with_args`，既有 `spawn` 不动）。
3. **Windows 实现**（Windows 专属文件，规则禁止手改）：`windows.rs` 改用共享的纯函数
   `decide_shell_args`，并在 `spawn_with_args` 里使用返回的 args。

三者缺一，Windows 上"用户 args 优先"就不会生效；因此本阶段只交补丁文本，实机效果由 Windows 3U 验证
（**待验证假设**）。

### 2.4 默认 shell 是什么（已验证事实）

`crates/app/src/lib.rs:74-79`：`terminal.shell.program` 显式配置优先 → 环境变量 `SHELL` →
平台默认（Unix `/bin/sh`、Windows `cmd.exe`）。opts 优先级在 `App::new`（`lib.rs:374-383`）。

## 3. 交给 Windows 侧的最小补丁（`shared:` 提交）

### 3.1 共享纯函数（建议放 `crates/config/src/lib.rs`，`#[cfg(any(windows, test))]` 防死代码告警）

```rust
/// 用户配置的 shell 启动参数优先；仅在"未配置 + 解析出的 shell 为 cmd"时给默认编码页参数。
/// `configured` 为 `None` 表示未配置；`Some(vec![])` 表示显式空数组（不注入任何默认参数）。
#[cfg(any(windows, test))]
pub fn decide_shell_args(program: &str, configured: Option<&[String]>) -> Vec<String> {
    if let Some(a) = configured {
        return a.to_vec();
    }
    let base = program
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(program)
        .trim_end_matches(|c: char| c == '\u{0}');
    let stem = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base);
    if stem.eq_ignore_ascii_case("cmd") {
        // 与现状逐字节一致（3N/3T 已验证 cmd 需要它才能 UTF-8）
        vec!["/K".to_string(), "chcp 65001".to_string()]
    } else {
        Vec::new()
    }
}
```

表驱动单测（输入 program × configured，期望值）：

| program | configured | 期望 |
|---|---|---|
| （未配置/空串，调用方已回落默认）`cmd.exe` | `None` | `["/K", "chcp 65001"]` |
| `cmd` | `None` | 同上 |
| `cmd.exe` | `None` | 同上 |
| `CMD.EXE` | `None` | 同上 |
| `C:\Windows\System32\cmd.exe` | `None` | 同上 |
| `pwsh` / `pwsh.exe` / `POWERSHELL.EXE` | `None` | `[]` |
| `/bin/bash` | `None` | `[]` |
| 任意（含 `cmd.exe`） | `Some([])` | `[]`（显式空数组不被覆盖） |
| 任意（含 `cmd.exe`） | `Some(["/D","/K","x"])` | 原样 `["/D","/K","x"]` |

### 3.2 Pty trait（shared，**需用户批准**，因为规则写的是"现有签名不得改"）

既有 `spawn` 不动，新增带默认实现的方法（与 4H 的 `take_writer()` 同一先例）：

```rust
/// 带显式启动参数（terminal.shell.args）的 spawn；默认忽略 args，走既有 spawn。
fn spawn_with_args(
    &mut self,
    shell: &str,
    args: &[String],
    cols: u16,
    rows: u16,
) -> Result<(), Error> {
    let _ = args;
    self.spawn(shell, cols, rows)
}
```

Unix 侧无需覆盖（`/bin/sh`、`bash` 不注入任何默认参数，等价于忽略）；Windows 侧覆盖它，
把 `for a in Self::utf8_codepage_args(shell)` 换成 `for a in decide_shell_args(shell, configured)`
（`configured` 由 app 从 `config.terminal.shell.args` 以 `Option<&[String]>` 传入）。

### 3.3 app 侧一行改动

`crates/app/src/lib.rs:1212` 改为调用 `spawn_with_args(&config.shell, &shell_args, cols, rows)`，
其中 `shell_args` 来自 `config.terminal.shell.as_ref().and_then(|s| s.args.as_deref())`。

## 4. t3 门禁对齐（已验证事实）

3T 已把两个 Windows pty 文件过 rustfmt，本机 `rustfmt 1.10.0-stable` 检查全仓：

```
$ cargo fmt --all --check
（无输出 → 0 偏差文件）
```

→ `scripts/gates.sh` 第 9 步去掉了"排除两个 Windows 文件"的逻辑，改为全仓检查。自检：
临时把 `crates/app/src/follow.rs` 改坏后该步打印 `Diff in …` 并失败，恢复后通过。

## 5. t4 文档更正（4S §3 归因）

- README「已知限制」原条目"Windows 滚动条列疑似色彩管线差异 1.46% ≈ 12/800=1.5%"改为：
  960 宽实测差异 **1.0304%**，滚动条列仅 **9 px（0.156%）**、列外字形区 **4739 px（≈1.03%）**，
  差异主要在**字形光栅**；Linux 未复现（NVIDIA/lavapipe 列内差异 0）。
- `docs/DECISIONS.md` 的 4S t3 条目按"删除线 + 追加更正（注明日期与来源）"处理。
- `PHASE4S-LINUX.md` §3 与 `PHASE4U-LINUX.md` §4.4 加更正注记，原文保留备查。
- 全仓 `grep "1.46" / "1.5%"` 已核对：README、DECISIONS、PHASE4U 三处已更正；`PHASE3Q-WIN.md`
  是当时观测记录（1.46% 数据本身无误）、`PHASE3R-WIN.md` 是 Windows 侧中间结论，均保留原样。

## 6. 测试与门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | 193 passed / 0 failed（本阶段无产品代码改动，仅文档与门禁脚本）|
| `cargo fmt --all --check` | 0 偏差（全仓，含两个 Windows pty 文件）|
| e2e `run_all.sh` | 110/110，GPU 轮 10/10 |
| Windows 目标 `cargo check` | 0 error / 0 warning（gates 第 7 步）|
| `scripts/gates.sh` | 9 步全过 |

## 7. 未解决 / 待验证

1. **`terminal.shell.args` 接线未实现**（待验证假设）：需 Windows 侧按 §3 实施并实测；
   指点规则本身的纯函数逻辑已在 §3.1 给出但**未落地为代码**（因覆盖点在 Windows 专属文件，
   且接线需动 trait，均与 4W 规则冲突）。
2. **Windows 字形光栅差异的机制未查**（工程判断）：仅知差异不在滚动条列、而在字形区；
   可能与 WARP 的 sRGB/混合路径有关，需 Windows 侧逐列统计。
3. `ShellConfig.args` 目前把"未配置"与"空数组"折叠，接线的第一步改动会影响 `config` 的公开
   结构（`Option<Vec<String>>`），届时需同步更新配置单测的期望值。

## 8. 手动项

1. Windows 侧 3U：按 §3 实施 `decide_shell_args` + `spawn_with_args` 并实测"用户 args 优先"、
   `cmd` 默认值不回归（UTF-8 中文仍正常）。
2. 若同意新增 `spawn_with_args`（带默认实现）触碰 Pty trait 表面，请先明确授权；否则需换方案
   （例如把 args 编码进 shell 字符串，但会破坏含空格路径的语义，不推荐）。
