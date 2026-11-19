# PHASE9A 发布包（0.2.2）

- Windows zip：`leantty-0.2.2-windows-x86_64.zip` **5,793,973 B**、sha256 **`f80d82fe6f5e26da7eb4f8e746a957fdbf6e7e6fbb71b771fa2355001ce70538`**；单顶层目录 10 条目；
  README.md `9bd64fea…`、README.zh-CN.md `80bdcd63…`、CHANGELOG.md `40cfa656…`、LICENSE `45c88805…`、
  THIRD-PARTY-NOTICES.md `cd28e4b0…`、leantty.toml `8824111b…` —— 与 rel-0.2.2 **6/6 一致**。
- deb：`leantty_0.2.2_amd64.deb` **3,519,904 B**、sha256 `5d32d944301edac09db511ac2d56d6bf902f86e22a093003ace3c7398dbc1c3b`；
  `Version: 0.2.2`、`Maintainer: leantty <noreply@example.invalid>`、Depends 9 项 + `Recommends: libvulkan1`；
  包内含 `changelog.Debian.gz`（0.2.2 条目）；包内 `/usr/bin/leantty` = `2b1defee…`，两次构建一致；
  deb 包本身因 changelog 时间戳不可复现（3,519,904 vs 3,523,452 B，已知）。
- 导出：169 文件 / 4.0 MB，`version = 0.2.2`；scan **A=0、C=0**、B=3（`crates/pty/src/windows.rs` 的系统路径常量）、
  豁免 15、仅报告 2。0.2.1 的导出目录已备份为 `leantty-release-0.2.1-backup`。
- 资产目录：`/root/dev/leantty-release-assets-9a/`，`SHA256SUMS` 仅含两文件。
- 0.2.1 → 0.2.2 差异：`Cargo.toml`/`Cargo.lock`（版本）、`CHANGELOG.md`（0.2.2 Fixed）、
  `crates/app/src/lib.rs`（字体就绪后置 urgent_render + request_redraw，含 debug-only 延迟钩子）、
  `scripts/e2e/run_all.sh`（新增 case81）。

## 0.2.2 重打（GPU 字形修复后，2026-11-19）

- 修复提交：`5675ac7`（GPU `add_background_fonts` 同时清 atlas，且 `GlyphAtlas::clear(queue)`
  **保留同一张纹理**——若用 `reset()` 会新建 texture/view 而绑定组仍是旧的，所有字形会采样空纹理）。
- 验证：同配置（size=16 / Consolas + HYQiHei）下 **CPU 与 GPU 就绪后截图逐像素一致（差 0）**；
  `case81` 增强为 3 项（CPU/GPU 各自前后变化 + CPU 与 GPU 一致），gates 10 步全过、e2e 131/131。
- tag：`phase9a-done2` = `5675ac7`（轻量；`phase9a-done` = `7628b13` 保留为第一版"仅重绘"修复）。
- 重新打包：`leantty_0.2.2_amd64.deb` **3,523,336 B**、sha256 `610543d0977854d74c886a98462b971c53b3b910bc8a4790027aa71614616124`，
  包内 `/usr/bin/leantty` = `568860f8…`，`changelog.Debian.gz` 为 0.2.2 条目；导出 169 文件、`version = 0.2.2`、
  scan A=0/C=0/B=3。
- **注意**：`/root/dev/leantty-release-inputs/leantty-0.2.2-windows-x86_64.zip`（`f80d82fe…`）是
  按 `phase9a-done` 打的，**不含本次 GPU 字形修复**，需 Windows 侧 9B 合并 `phase9a-done2` 后重打。
