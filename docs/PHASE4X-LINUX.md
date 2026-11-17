# PHASE4X-LINUX：合并 3U 审计 + Unix 侧 `shell.args` 生效 + 收口交付

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并审计 | `phase3u-win-done` 轻量 tag `9b565be`；`crates/` 4 个文件、`render`/`vt`/`vendor` 零改动；**`Pty` trait 既有方法一字未动**，只新增带默认实现的 `spawn_with_args`；合并无冲突 → gates 全过 → tag `phase4x-t1`（`31437e8`）|
| t2 Unix args | `unix.rs` 实现 `spawn_with_args`，`spawn` 改为委托；**回退即红**；e2e 新增 case53/54/55（cpu+gpu 各一轮）全过，e2e 110 → **113** |
| t3 Windows 构建 | 仅 `unix.rs`+18 与 `config/tests/config.rs`+52（测试目标）；判 RC2 功能等价、可继续作为最终包，严格字节对应建议 3V 重建 |
| t4 文档 | README（args 三态与默认值、已知限制整合）、`PROJECT-SUMMARY.md`、本文件 |
| t5 交付包 | 见 §6（deploy/dev 两包 + 空目录冒烟 + sha256）|
| t6 收尾 | gates 全过、e2e 113/113、tag `phase4x-done` |

## 1. t1 合并与审计

### 1.1 tag 性质（`git ls-remote` 实际输出）

```
79fbb0ee9ccc22f11a0f1bcb7844cc8250a86b40	refs/heads/main
9b565bee47f95bd49350c2076b607a39028021d6	refs/heads/win
9b565bee47f95bd49350c2076b607a39028021d6	refs/tags/phase3u-win-done
79fbb0ee9ccc22f11a0f1bcb7844cc8250a86b40	refs/tags/phase4w-done
```

`phase3u-win-done` 与 `refs/heads/win` 同 hash 且**没有 `^{}` 行** → 轻量 tag。

### 1.2 合并前范围核查

```
$ git diff --stat phase4w-done phase3u-win-done -- crates/
 crates/app/src/lib.rs     |  14 ++++-
 crates/config/src/lib.rs  |  11 ++--
 crates/pty/src/lib.rs     |  14 +++++
 crates/pty/src/windows.rs | 144 +++++++++++++++++++++++++++++++++-------------
 4 files changed, 137 insertions(+), 46 deletions(-)
```

`git diff --stat … -- crates/render/ crates/vt/ vendor/` 为空（零改动）。

`crates/pty/src/lib.rs` 的完整 diff：**只在 `spawn` 之后插入了带默认实现的
`spawn_with_args(&mut self, shell, args: Option<&[String]>, cols, rows)`**，默认实现
`let _ = args; self.spawn(shell, cols, rows)`；既有 `spawn`/`take_reader`/`writer` 签名与语义未动。

`crates/config/src/lib.rs`：`ShellConfig.args: Vec<String>` → `Option<Vec<String>>`，
`args.unwrap_or_default()` → `args`（解析函数本来就产出 `Option`：非数组 → 告警 + `None`，
非字符串元素 `filter_map` 静默丢弃），因此 **TOML 解析语义未变**；Linux 侧该字段此前未被消费，
既有行为不受影响。

`crates/app/src/lib.rs`：spawn 调用改为 `pty.spawn_with_args(&config.shell, shell_args.as_deref(),
cols, rows)`，`shell_args` 取 `config.terminal.shell.args`。

### 1.3 发现的缺口（已补）

3U **没有为 `args` 的新语义带任何测试**（全仓 `grep args` 只命中键名与告警文本）。4X t2 补了
`crates/config/tests/config.rs::shell_args_distinguishes_absent_empty_and_invalid`，覆盖四种输入形态。

### 1.4 合并

```
$ git merge --no-ff phase3u-win-done^{commit} -m "merge: phase3u-win-done"
（无冲突；合并提交 31437e8）
```

合并后 `git diff --stat HEAD phase3u-win-done -- crates/` 为空。gates 9 步全过（e2e 110/110）→
`phase4x-t1`。

## 2. t2 Unix 侧 `terminal.shell.args` 生效

### 2.1 实现（已验证事实）

```rust
fn spawn(&mut self, shell: &str, cols: u16, rows: u16) -> Result<(), Error> {
    // 等价于"未配置 args"。既有语义与字节序列均不变。
    self.spawn_with_args(shell, None, cols, rows)
}

fn spawn_with_args(&mut self, shell: &str, args: Option<&[String]>, cols: u16, rows: u16)
    -> Result<(), Error> { /* 既有实现 + */ if let Some(list) = args { for a in list { cmd.arg(a); } } }
```

### 2.2 注入确实生效的证据（e2e，cpu + gpu 各一轮）

标记文件路径由脚本的 `OUT` 目录推导（`scripts/e2e/out/shellargs/…`），不硬编码绝对路径：

```
  [c53 cpu] /bin/sh -c 'echo ok > 标记; exec sleep 30' → 标记=ok 窗口=存活
  [c53 gpu] /bin/sh -c 'echo ok > 标记; exec sleep 30' → 标记=ok 窗口=存活
PASS case53-shell-args (2/2)
  [c54 none/cpu] 未配置 args → 键入标记=typed 进程=存活
  [c54 none/gpu] 未配置 args → 键入标记=typed 进程=存活
PASS case54-shell-args-none (2/2)
  [c55 empty/cpu] args = [] → 键入标记=typed 进程=存活
  [c55 empty/gpu] args = [] → 键入标记=typed 进程=存活
PASS case55-shell-args-empty (2/2)
```

**回退验证**（临时删掉 `unix.rs` 的覆盖、让 trait 默认实现生效后重建）：

```
标记=无 进程=存活      ← case53 等价测量红（args 被忽略，标记没写出来）
（恢复后）标记=ok 进程=存活
```

### 2.3 Windows 目标编译

`cargo check --target x86_64-pc-windows-gnu --workspace --tests` → **0 error / 0 warning**。
说明：`crates/pty/src/lib.rs:12` 的 `pub mod unix` **没有 cfg 门控**（该模块只用 portable-pty
跨平台 API），因此 unix.rs 也会参与 Windows 编译；但 Windows 上 `NativePty = WindowsPty`，
`UnixPty` 不被实例化，行为不受影响。

## 3. t3 Windows 构建有效性判定

```
$ git diff --stat phase3u-win-done HEAD -- crates/
 crates/config/tests/config.rs | 52 +++++++++++++++++
 crates/pty/src/unix.rs        | 18 ++++++
 2 files changed, 70 insertions(+)
```

- `crates/pty/src/unix.rs`：只服务 Unix 实例化路径（Windows 上 `UnixPty` 不被使用），但**会参与
  Windows 编译单元**。
- `crates/config/tests/config.rs`：测试目标，不进入 release 二进制。

**判定（工程判断，置信度高）**：Windows RC2（zip 6607474 B，sha256
`868ce25430cc12646282de5a15715a4b5d01c31972e150be8615159828149f2f`，数值由 Windows 侧自述、
Linux 侧**未核实**）在**功能上**与最终源码一致，可继续作为最终 Windows 包；如要求产物与源码
严格一一对应（可复现构建），建议 Windows 3V 用 `phase4x-done` 重建一次。
推翻条件：Windows 侧重建的产物哈希与 RC2 不同，且差异可归因到 unix.rs 参与编译。

## 4. t4 文档

- README：`terminal.shell.args` 改为三态说明（未配置 / 显式空数组 / 具体参数）、Linux 与 Windows
  均生效、Windows 未配置时的默认值（`cmd.exe`、`powershell.exe` 有 chcp 默认值，`pwsh` 与其他无）、
  显式空数组不注入默认、`pwsh` 中文输出影响**未核实**。
- README：`已知限制`合并为四组（平台与部署 / 渲染 / 交互与输入 / 未定位的异常），逐条标出处。
- 新增 `docs/PROJECT-SUMMARY.md`（阶段 0→4X 结论性总结与挂起项）。

## 5. 测试与门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **194 passed / 0 failed**（新增 1 条 config args 语义测试）|
| e2e `run_all.sh` | **113/113**，GPU 轮 10/10 |
| Windows 目标 `cargo check --workspace --tests` | 0 error / 0 warning |
| `scripts/gates.sh` | 9 步全过（含全仓 rustfmt、clippy 两配置、release 零告警、wgpu==30.0.2）|

## 6. t5 Linux 交付包（dual-package skill）

产物放在项目目录之外（`/root/dev/tty_packages/`，未进仓库）：

| 包 | 大小 | sha256 |
|---|---|---|
| `tty.tar.gz`（部署包，顶层目录 `tty/`） | 5,359,817 B | `7ce62c6d1801e1dde852b6377a2db06f522e871435e1704f6616f9b148e2934f` |
| `tty_dev.tar.gz`（开发包，顶层目录 `tty_dev/`） | 4,362,182 B | `01c59f1ad107593c2c1aed01c5aea8967a7688f91638ccd6fca1ce3c1a481128` |

部署包内容：`app`（16,043,792 B 预编译二进制）+ `config.example.toml` + `DEPLOY.md`
（含"第 1 步防覆盖解压 `tar -xzkf`"、依赖表、配置项表、已知问题）。
开发包内容：`git archive HEAD` 快照（1789 个文件，18MB 解压）+ 追加的 `CHANGELOG.md`
（含本轮决策/未决项/待验证假设）+ 构建须知（外部 zig 与 ghostty 源码目录以环境变量注入）。

**部署包冒烟（含空格的临时目录）**：

```
$ mkdir -p "/root/dev/验证 目录 with spaces/部署" && tar -xzkf tty.tar.gz -C "…"
$ ldd app
  linux-vdso.so.1 / libc.so.6 / ld-linux-x86-64.so.2 / libgcc_s.so.1 / libm.so.6
$ DISPLAY=:99 APP_CONFIG=… SHELL=/bin/sh ./app
  窗口=创建成功 首帧=first_frame_ms=191 截图=PNG 1280x720（均值亮度 0.0227，非全黑）
  SIGTERM 后退出码=143（20ms 内退出）
```

X11 与 Vulkan 都是**运行期 dlopen**（`ldd` 只见 libc/libm/libgcc），因此部署机缺 Vulkan 时
`backend = "cpu"`/`"auto"` 仍可用。

**引用完整性扫描**（skill 的 `check_refs.sh`）：部署包 **0 处未解析引用**；开发包 43 处，全部位于
历史阶段报告（`docs/DECISIONS.md` 12、`docs/WIN-NOTES.md` 6、`vt-verify/REPORT.md` 4、
`PHASE3A-WIN.md` 3 等），指向仓库外路径（vendored wgpu 源码、Windows 侧工作目录、ghostty 头文件）
或纯文本里的数字斜杠（`96dpi/1.0`、`323.6/333.7/330.1`）。**未改写源项目**（本阶段"只改必需部分"），
归为开发包的已知项：这些是历史报告的外部引用，不影响构建与运行。

## 7. 未解决 / 待验证

1. **Windows 实机效果未验证**（待验证假设）：`args` 在 Windows 上"用户配置优先"由 3U 自测，
   本阶段只在 Linux 侧证明 Unix 路径；Windows 侧中文输出（尤其 `pwsh` 无默认 chcp）**未核实**。
2. **RC2 与最终源码的字节级对应关系未核实**（见 §3）。
3. 挂起项（不在本阶段处理）：`load_font_list` 深拷贝、GPU 初始化前移、适配器选择正式配置、
   CPU soak 提前退出、GPU 自动滚动 605 行异常。
