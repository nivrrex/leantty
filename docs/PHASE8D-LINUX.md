# PHASE8D-LINUX：0.2.1 发布包（deb / 导出 / 资产）

## D1 发布基线（已验证事实）

- 源码基线：`phase8a-done` = `bc5735c61b012a679c1a9491061ac17b8b7fe521`（`Cargo.toml version = 0.2.1`）。
- `refs/tags/phase8c-win-done` = `86587cbfe25571bbac3242ee437357ce73392410`，与任务书一致；`--no-ff` 合并为 `be4a77f`。
- `git diff --stat phase8a-done HEAD -- crates/ Cargo.toml Cargo.lock vendor/` **为空** → 合并未带入任何 Linux 侧代码变化；
  另以 `grep` 确认 `crates/app/src/lib.rs` 中**没有** 8B 的观测改造（`read_n=` / `large_drain_hits` / `[perf] final` 计数为 0）。
- 冲突：`docs/DECISIONS.md` 双方保留（Windows 侧 8C 条目 + Linux 侧 8A/8B 条目）。

## D2 Windows zip 核对（已验证事实）

`leantty-0.2.1-windows-x86_64.zip`：**5,793,626 B**、sha256 **`25d47aa95f5d215b3e0d6f4bf41581266abeddd489c084c6be7237ba08cd9ec3`**，
单顶层目录 10 条目。逐文件（去 CRLF）与 rel-0.2.1 比对**全部一致**：
README.md `9bd64fea…`、README.zh-CN.md `80bdcd63…`、CHANGELOG.md `22bcf202…`、LICENSE `45c88805…`、
THIRD-PARTY-NOTICES.md `cd28e4b0…`、leantty.toml（对 `config/leantty.windows.toml`）`8824111b…`。

## D3 门禁与 deb（已验证事实）

- 在 rel-0.2.1 上 `scripts/gates.sh` **10 步全过**：`GATES: PASS — HEAD=be4a77f`，e2e **130/130**、GPU 轮 10/10。
- `leantty_0.2.1_amd64.deb`：**3,520,304 B**、sha256 `5888a6294158a729ea85c5e75be531cbb2897b8c96660381dfcad9eecf4a5315`；
  `Version: 0.2.1`；`Maintainer: leantty <noreply@example.invalid>`（占位）；
  `Depends: libc6, libwayland-client0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0`；
  `Recommends: libvulkan1`。
- 包内文档：README.md / README.zh-CN.md / CHANGELOG.md / THIRD-PARTY-NOTICES.md / copyright /
  **changelog.Debian.gz（0.2.1 条目，已确认存在）** / examples/leantty.toml；`/usr/bin/leantty` = `42757fc1…`。
- 未在本机 `dpkg -i`。

## D4 导出与扫描（已验证事实）

`scripts/release/export.sh /root/dev/leantty-release` → **169 文件 / 4.0 MB**，`Cargo.toml version = 0.2.1`。
`scan.sh`：**A=0**、**C=0**、B=3（`crates/pty/src/windows.rs` 的 `C:\Windows\System32` 等系统常量）、
豁免 15、仅报告 2（两处 "LTSC"）。0.2.0 的导出目录已备份到 `leantty-release-0.2.0-backup`（含当时的 `.git`，
该目录原本无 `.git`，未删除用户数据）。

## D5 资产（已验证事实）

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.1_amd64.deb` | 3,520,304 B | `5888a629…eecf4a5315` |
| `leantty-0.2.1-windows-x86_64.zip` | 5,793,626 B | `25d47aa9…08cd9ec3` |
| `SHA256SUMS` | 189 B | 仅两个文件名 |

deb 包本身因 `changelog.Debian.gz` 内嵌构建时间戳不可复现（已知）；包内二进制与源码对应。

## D6 0.2.0 → 0.2.1 逐文件差异（12 个）

`Cargo.toml`/`Cargo.lock`（版本 0.2.0→0.2.1）、`CHANGELOG.md`（0.2.1 段：流式选区、单击不留标记、
跨短行拖选修复）、`crates/app/src/lib.rs`、`crates/app/src/selection.rs`、`crates/render/src/decide.rs`、
`crates/render/tests/incremental.rs`（流式选区的判定与测试语义适配）、`README.md`、`README.zh-CN.md`、
`scripts/release/build-deb.sh`（changelog 写入位置修正）、`scripts/windows/conpty/fetch-conpty.ps1`、
`scripts/windows/package.ps1`（Windows 侧合并带入）。

## D7 收口（已验证事实）

轻量 tag **`phase8d-done`** = `be4a77fe6613e07130df5cc1d593a2f0e5520719`（经 `tag_checked.sh`，
`^{}` 行数 0）；push 了分支 `rel-0.2.1` 与该 tag，**main 未动**（仍 `b7567b4`）。
