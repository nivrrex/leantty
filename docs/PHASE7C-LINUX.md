# PHASE7C-LINUX：hinting 默认值修复、合并 7B、0.2.0 交付

## H0 取证（已验证事实）

| 项 | 结果 |
|---|---|
| 打印位置 | `crates/render/src/lib.rs:552-554`，条件 `options.hinting` |
| 默认值 | `crates/config/src/lib.rs:380` 与 `crates/render/src/lib.rs:155` 均为 `true` → **默认配置启动即打印** |
| 模板 | 两份模板显式 `hinting = false`（`config/leantty.linux.toml:48`、`config/leantty.windows.toml:48`），用模板启动 stderr 为空 |

判定：默认触发属缺陷。用户裁定方案 (3)：默认值改为 `false`。

## H1–H4 修复（已验证事实）

- **H1 只读核查**：`hinting` 只出现在 `RenderOptions::fingerprint()` 的字形缓存键（`lib.rs:164-176`）
  与那条提示日志，**不改变渲染像素**（fontdue 未实现 hinting）。测试里 3 处 `hinting: false` 是构造用。
- **H2**：`crates/config/src/lib.rs:380`、`crates/render/src/lib.rs:155` 默认值改为 `false`；
  模板注释改为 "The rasterizer has no hinting support; setting this to true only prints a notice."；
  README 两份新增一句（`font.hinting` 兼容保留、不生效、显式开启会打印一行提示）。
- **H3 证据**：默认配置启动 stderr **空**；显式 `hinting = true` 仍打印该行（新增 e2e `case75` 断言两者）；
  回退默认值（改回 true）即 case75 的 default 分支变红（H0 观测即该状态）。
  黄金哈希 `frame_bytes_identical` 仍 **507485fb7945fb8f**，workspace **209 测试通过**。
- **H4**：`CHANGELOG.md` 0.2.0 的 Changed 增一条（用户视角）。

## C1 合并 7B（已验证事实）

```
$ git ls-remote ali | grep -E "refs/heads/(main|win)$|phase7b-win-done"
b9c4f200c7f18a385e59fcd463acb745f2575963	refs/heads/win
b9c4f200c7f18a385e59fcd463acb745f2575963	refs/tags/phase7b-win-done   ← 轻量 tag
```
与任务书一致。`--no-ff` 合并；冲突仅在 `docs/DECISIONS.md`（双方文件尾追加），按双方保留解决；
`README.md`/`README.zh-CN.md` 自动合并。合并提交 `a32cd66`。

合并后 `crates/` 相对 `phase7a-done` 的全部差异（4 文件）：

| 文件 | 变化 | 来源 |
|---|---|---|
| `crates/pty/src/windows.rs` | +7：显式 `cmd.cwd(current_dir())`（Windows 侧同款 cwd 修复）| 7B（Windows）|
| `crates/pty/tests/pty_windows.rs` | +32：`spawn_child_inherits_process_cwd` 等回归测试 | 7B（Windows）|
| `crates/config/src/lib.rs` | 1 行：hinting 默认 false | 7C H2 |
| `crates/render/src/lib.rs` | 2 行：hinting 默认 false + 文档 | 7C H2 |

## C2 Windows zip 核对（已核对，重打包后全部一致）

zip：`/root/dev/leantty-release-inputs/leantty-0.2.0-windows-x86_64.zip`（Windows 侧重打包版本）
- 大小 **5,791,602 B**、sha256 **`a60b16433d561c787abaea6306732cebf2920f9b31613de92b1c870ef5a3a965`**
- 顶层 `leantty-0.2.0-windows-x86_64/`，10 条目（leantty.exe、conpty.dll、OpenConsole.exe、leantty.toml、
  leantty-portable.cmd、LICENSE、THIRD-PARTY-NOTICES.md、README.md、README.zh-CN.md、CHANGELOG.md）

逐文件与仓库比对（**全部一致**）：

| 文件 | sha256（前 16 位）| 结论 |
|---|---|---|
| README.md | `a3eac279f1e5a76d` | 一致（字节级也一致，无 CRLF 差异）|
| README.zh-CN.md | `1c9ea674806355ea` | 一致 |
| CHANGELOG.md | `e20c6113b6548458` | 一致 |
| LICENSE | `45c88805fba0e684` | 一致 |
| THIRD-PARTY-NOTICES.md | `cd28e4b0c6324e26` | 一致 |
| leantty.toml（对 `config/leantty.windows.toml`）| `8824111bd1fda252` | 一致 |

（首版 zip `01faa5e7…` / 5,791,390 B 曾滞后于 H2/H4 的文档改动，已由 Windows 侧重打包取代。）

## C3 deb 0.2.0（已验证事实）

`scripts/gates.sh` 10 步全过（`GATES: PASS — HEAD=a32cd66`，e2e 128/128）后构建：
- `leantty_0.2.0_amd64.deb` **3,520,632 B**，sha256 `9b51a2d4411ae6eaceea3de7f3e07fb7ffb477f133131ba25b1d293e4b137915`
  （首版 3,516,768 B / `a823f5b0…` 缺 `changelog.Debian.gz`，见下方缺陷与修复）
- `Version: 0.2.0`（脚本无硬编码）；`Maintainer: leantty <noreply@example.invalid>`（占位）
- `Depends: libc6, libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0`；`Recommends: libvulkan1`
- 内容清单含 `/usr/bin/leantty`、desktop、hicolor 8 尺寸 + scalable SVG、doc 下 README/README.zh-CH/CHANGELOG/THIRD-PARTY-NOTICES/copyright/changelog.Debian.gz/examples/leantty.toml
- 未在本机 `dpkg -i`。
- **两次构建的包内二进制 sha256 相同**（`f9725440…`）；deb 包本身不同（`a823f5b0…` vs `f82429d1…`，3,516,768 vs 3,520,892 B）——`changelog.Debian.gz` 内嵌构建时间戳，属已知不可复现项。

## C4 导出与扫描（已验证事实）

`/root/dev/leantty-release`：**169 文件 / 4.0 MB**；扫描 **A=0**、**C=0**、B=3（`crates/pty/src/windows.rs`
的 `C:\Windows\System32` 等系统常量）、豁免 15、仅报告 2（两处 "LTSC" 平台说明）。

## C5 资产（已验证事实）

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.0_amd64.deb` | 3,520,632 B | `9b51a2d4411ae6eaceea3de7f3e07fb7ffb477f133131ba25b1d293e4b137915` |
| `leantty-0.2.0-windows-x86_64.zip` | 5,791,602 B | `a60b16433d561c787abaea6306732cebf2920f9b31613de92b1c870ef5a3a965` |
| `SHA256SUMS` | 189 B | 仅两个文件名 |

## C6 0.1.1 → 0.2.0 逐文件差异（15 个文件，无增删）

| 文件 | 摘要 |
|---|---|
| `Cargo.toml` / `Cargo.lock` | 版本 0.1.1 → 0.2.0 |
| `CHANGELOG.md` | 新增 0.2.0 段（Added：`-e/--command`、`--working-directory`；Changed：默认工作目录改为启动目录、hinting 启动提示不再默认出现）|
| `config/leantty.{linux,windows}.toml` | hinting 注释改写（语义不变）|
| `crates/app/src/cli.rs` | 新增 `-e/--command`、`--working-directory` 解析与单测 |
| `crates/app/src/main.rs` | 路径绝对化、目录校验、`set_current_dir`、错误码 |
| `crates/app/src/lib.rs` | `exec_command` 覆盖、spawn 时使用、`hinting` 传递 |
| `crates/config/src/lib.rs` | hinting 默认 false |
| `crates/render/src/lib.rs` | hinting 默认 false + 文档 |
| `crates/pty/src/unix.rs` | 显式 `cmd.cwd(current_dir())`（替代骨架期的 `/tmp`）|
| `crates/pty/src/windows.rs` | Windows 侧同款 cwd 修复（7B）|
| `crates/pty/tests/pty_windows.rs` | Windows cwd 回归测试（7B）|
| `README.md` / `README.zh-CN.md` | 新选项、示例、已知限制、hinting 说明 |

## C8 打包缺陷与修复（已验证事实）

`scripts/release/build-deb.sh` 原先把 `changelog.Debian.gz` 写到 `$PKGROOT` 下，而该目录在写 changelog
**之前**已经被 `cp -a "$PKGROOT/." "$WORK/debian-tmp/"` 复制走 → **changelog.Debian.gz 从未进入 deb**
（0.1.1 与 0.2.0 首版均缺，`dpkg-deb -c` 可证）。

修复：改为直接写入 `$WORK/debian-tmp/usr/share/doc/leantty/changelog.Debian{,.gz}`。
重建后 `dpkg-deb -c` 含 `changelog.Debian.gz`，内容为 0.2.0 条目；包内二进制仍为 `f9725440…`；
新 deb = **3,520,632 B** / sha256 **`9b51a2d4…`**；资产目录与 `SHA256SUMS` 同步更新。
