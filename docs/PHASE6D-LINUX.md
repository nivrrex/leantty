# PHASE6D-LINUX：合并 6D-win 修复、Windows zip 核对、deb 0.1.1、导出与资产

## 1. 合并（已验证事实）

```
$ git ls-remote ali | grep -E "refs/heads/(main|win)$|phase6d"
3485a10b8601ef11683f2ce5bcc1e309af375776	refs/heads/main
95752294fdb2d6d018bb771005619399ef2a214f	refs/heads/win
95752294fdb2d6d018bb771005619399ef2a214f	refs/tags/phase6d-win-fix      ← 轻量 tag（无 ^{} 行）
```

两者与任务书一致。`--no-ff` 合并该提交，`docs/DECISIONS.md` 出现一次内容冲突（双方都在文件尾追加），
按"双方都保留"解决（Linux 6B 条目 + Windows 6C/6D 条目），合并提交 `6c79b2a`。

### 1.1 `crates/` 相对 `phase6b-done2`(4b79612) 的差异（已验证事实）

只有 **1 个文件**：`crates/app/src/lib.rs` **+110 行**，全部位于
`#[cfg(all(test, windows))] mod keymap_tests`（Windows 键映射单测：`map_logical_key` 的
F1–F12/Insert/Delete/PageUp/PageDown/符号/字母数字，以及默认 Shift+PageUp/PageDown→滚动绑定）。
Linux 构建与运行行为不受影响；`Cargo.toml`/`Cargo.lock`/`vendor/` 零改动。

## 2. Windows zip 核对（已核对）

zip：`/root/dev/leantty-release-inputs/leantty-0.1.1-windows-x86_64.zip`
- 大小 **5,786,634 B**、sha256 **`1713d3f5c01a5348cb387956a392ff7d69aba44264ff23230e6b2c79a829132a`**，与任务书一致。
- 单顶层目录 `leantty-0.1.1-windows-x86_64/`，10 个条目（leantty.exe、conpty.dll、OpenConsole.exe、
  leantty.toml、leantty-portable.cmd、LICENSE、THIRD-PARTY-NOTICES.md、README.md、README.zh-CN.md、CHANGELOG.md）。

逐文件哈希（zip vs 合并后仓库）：

| 文件 | 结果 |
|---|---|
| CHANGELOG.md | 一致（`68903d9f…`）|
| LICENSE | 一致（`45c88805…`）|
| THIRD-PARTY-NOTICES.md | 一致（`cd28e4b0…`）|
| leantty.toml（对 `config/leantty.windows.toml`）| 一致（`623fc939…`）|
| README.md | 字节不同，但**去掉 CRLF 后完全一致**（zip 内为 Windows 行尾）|
| README.zh-CN.md | 同上（CRLF 差异，内容一致）|

## 3. deb 0.1.1（已验证事实）

`scripts/gates.sh` 10 步全过（`GATES: PASS — HEAD=6c79b2a`，e2e 119/119）之后构建：

- `leantty_0.1.1_amd64.deb` **3,516,304 B**，sha256 `031e49f6d6319984f9703de4e81057a4ea126f3045638c25c6b7e291b3457b46`
- `Version: 0.1.1`（版本来自 `Cargo.toml`，脚本无硬编码）
- `Depends: libc6, libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0`
- `Recommends: libvulkan1`
- `Maintainer: leantty <noreply@example.invalid>`（占位，需用户替换）
- 内容：`/usr/bin/leantty`、`leantty.desktop`、hicolor 8 尺寸 PNG + scalable SVG、
  `/usr/share/doc/leantty/{README.md,README.zh-CN.md,CHANGELOG.md,THIRD-PARTY-NOTICES.md,copyright,changelog.Debian.gz,examples/leantty.toml}`
- 未在本机 `dpkg -i`（按要求）。

**可复现性**：连续两次构建的 **deb 内 `/usr/bin/leantty` sha256 相同**（`2074d934…`，与 `target/release/leantty` 一致）；
**deb 包本身不同**（`031e49f6…` vs `a794ad2a…`，大小 3,516,304 vs 3,513,060）——原因是
`changelog.Debian.gz` 内嵌 `date -R` 构建时间戳。属预期，非源码差异。

## 4. 导出与扫描（已验证事实）

`scripts/release/export.sh` → `/root/dev/leantty-release`，**169 个文件 / 3.9 MB**。
`scripts/release/scan.sh` 全量扫描：**A=0**、B=3（`crates/pty/src/windows.rs` 的 `C:\Windows\System32` 等系统常量）、
**C=0**、豁免 15（单元测试合成路径、ping 目标回环、Windows 系统常量、vendor 上游文档）、
仅报告 2（`crates/app/src/lib.rs:120` 的 "Win10 LTSC" 与 `scripts/windows/conpty/README.md:7` 的 "Windows 10 LTSC 19044"）。

## 5. 发布资产（已验证事实）

`/root/dev/leantty-release-assets-6d/`：

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.1.1_amd64.deb` | 3,516,304 B | `031e49f6d6319984f9703de4e81057a4ea126f3045638c25c6b7e291b3457b46` |
| `leantty-0.1.1-windows-x86_64.zip` | 5,786,634 B | `1713d3f5c01a5348cb387956a392ff7d69aba44264ff23230e6b2c79a829132a` |
| `SHA256SUMS` | 189 B | 仅含上面两个文件名，不含路径 |

## 6. 0.1.0 → 0.1.1 逐文件差异（供审阅）

`diff -rq` 结果（11 处）：

| 变更 | 文件 | 摘要 |
|---|---|---|
| 改 | `Cargo.toml` | 版本 0.1.0 → 0.1.1 |
| 改 | `Cargo.lock` | 随之更新本地 crate 版本 |
| 增 | `CHANGELOG.md` | 新增（0.1.1 条目：kitty 键盘输入修复、功能键与 Ctrl+[、PageUp/PageDown 转发、启动提示默认不输出）|
| 改 | `crates/app/src/lib.rs` | 6B 键盘修复（F1–F12/Insert/Delete/符号键映射、控制字符文本走编码器、`key_without_modifiers`、`ctrl+[` 回写、PageUp/PageDown 转发、默认 Shift+PageUp/PageDown 滚动绑定、启动提示改 `eperf!`）+ 6D 合并带入的 `cfg(windows)` keymap 单测 |
| 改 | `crates/vt/src/encoder.rs` | 新增 `encode_key_full`（带 unshifted codepoint / utf8）|
| 增 | `crates/vt/tests/key_kitty.rs` | kitty 协议下 Ctrl+字母的编码回归（模式 × 键表驱动）|
| 改 | `README.md` / `README.zh-CN.md` | 6B 已知限制与键盘滚动说明、删除启动提示故障排查条目 |
| 改 | `scripts/release/build-deb.sh` | 文档区加入 CHANGELOG；changelog 条目改为 0.1.1 内容 |
| 改 | `scripts/windows/package-release.ps1` | 来自 Windows 侧（6D） |
| — | `.git` 目录 | 仅存在于 0.1.0 导出目录（当时用于首次 GitHub 上传），不在白名单内 |

## 7. 已知限制

沿用 6B 结论：部分 Ctrl 组合（Ctrl+Semicolon/Comma/Period/Apostrophe、Ctrl+Shift+Semicolon、Ctrl+I/M）
在 Linux 上仍无输入；不发送 kitty 键盘协议的按键释放事件；启动提示那条 `request_inner_size:` 默认不输出。
`NumLock` 开启时 Shift+PageUp 是否仍生效，仅在 Xvfb（无 NumLock 位）验证，**真实 X 会话待用户实测**。
