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

## 启动早期"方块 → 中文"约 1 秒的成因（实测拆解）

`:99`、用户配置（`size=16`、`normal=Consolas`、`fallback=HYQiHei`）、CPU 后端：

| 打点 | 值 |
|---|---|
| `first_frame_ms` | **306**（首帧画出来时中文还没就绪 → 方块）|
| `bg_db_new_us` | 1 µs |
| `bg_fontdb_us`（`load_system_fonts` 扫描系统字体）| **80 ms** |
| `bg_font_clone_us`（一个 CJK 面的 `deep_copy`）| **137.6 ms** |
| `bg_font_list_us`（后台加载总计）| **645.6 ms** |
| `bg_fonts_merge_us` | 9 µs |

即：首帧 306 ms 时先画方块，字体在 ~950 ms 才并入 → 方块持续约 **0.65 s**（用户体感"1 秒"）。
耗时主要在**解析 CJK 字体（`from_bytes`，约 0.43 s）**、扫描系统字体（80 ms）与一次**深拷贝**（137 ms）。
这是既有设计取舍：主字体同步加载、回退字体后台加载，避免把首帧拖慢到 1 秒以上。

**可选改法（需用户裁定）**
1. **并行提前**：把回退字体的加载在进程启动早期就发起（与建窗/建 GL 并行），并去掉那 137 ms 的深拷贝，
   首帧仍不等字体，但方块持续时间预计可从 ~650 ms 降到 ~200 ms（估计）。
2. **首帧等字体**：首帧前阻塞等待回退字体就绪，彻底没有方块，但窗口出现时间从 ~306 ms 推迟到 ~950 ms。
3. **保持现状**，仅文档说明。

## 启动方块期优化（已实施，`b785881` / tag `phase9a-perf`）

实测（`:99`、`size=16` / Consolas + HYQiHei、CPU 后端）：

| 指标 | 改前 | 改后 |
|---|---|---|
| `first_frame_ms` | 306 | **230** |
| `bg_font_list_us` | 645.6 ms | **510.9 ms** |
| `bg_fonts_ms`（并入时刻）| ~950 | **545** |
| **方块可见时长** | ~0.65 s | **~0.31 s（−52%）** |

做法（全部新增入口，既有签名未变）：
1. `render::load_font_list_shared` / `CpuRenderer::add_background_fonts_shared` /
   `GpuRenderer::add_background_fonts_shared`：后台线程本来按 `Arc<Font>` 缓存字体，改为一并交出去，
   省掉 `Vec<Font>` 的那次深拷贝（每 CJK 面 ≈137 ms）；旧入口保留并委托，行为不变。
2. 回退字体线程提前到 `resumed` 开头（**建窗之前**）发起，与建窗、建 surface、首帧完全重叠。

未达最初"≤200 ms"的估计：剩余 ~315 ms 由**后台线程自己的 fontdb 扫描（128 ms）**与 CJK 解析（~0.38 s）构成。
再压需要把主线程已建好的 `Database` 共享给后台线程（可省 ~128 ms，预计方块期 ≈150 ms），
但那要改 `CpuRenderer::new` / `create_renderer` 的签名，与"不改 Renderer 既有签名"的约束冲突，暂未做。

## Windows zip 定稿（9C 返工后）

- 来源：Windows 侧 `win` 分支 `2c3ff22`（合并 `origin/rel-0.2.2` 全部 6 个文档提交；唯一冲突 `docs/DECISIONS.md`，双方保留），
  由 `scripts/windows/package-release.ps1` 直接产出（脚本未改动）。
- 产物：`leantty-0.2.2-windows-x86_64.zip` **5,795,070 B**、
  sha256 `d3ae8a3d79eddb4f779bd1a1bf5f2562fe46bdbd79568746a75164bc5e02e5e9`，10 条目（前缀 `leantty-0.2.2-windows-x86_64/`）。
- Linux 侧复核（解压实测）：10 项 sha256 **全部与期望表一致**；`README.md`/`README.zh-CN.md`/`CHANGELOG.md` 与仓库 `5ec6b31` 逐字节一致；
  `leantty.toml` 与 `config/leantty.windows.toml` 一致；`leantty.exe` 12,802,560 B = `12f4b468…`（与合入前的构建相同，证明未混入源码改动）。
- 前一轮交付被判不合格的偏差已消除：条目数 6→10、配置名 `config.toml`→`leantty.toml`、README.zh-CN 由 `80bdcd63…`（b785881）更新为 `7a8f781d…`（含遗留问题条目）。
- 资产目录 `/root/dev/leantty-release-assets-9a2/` 与 `SHA256SUMS` 已更新为上述 zip 与 deb `dc8404ae…`。

## v0.2.2 发布（GitHub）

- 公开仓库 `nivrrex/leantty` 新增提交 `ddfd285` "leantty 0.2.2"（相对 `08b55b4` 改 15 文件：`Cargo.toml`/`Cargo.lock` 版本、`crates/app/src/{lib,selection}.rs`、
  `crates/render/src/{decide,gpu/atlas,gpu/mod,lib}.rs`、`crates/render/tests/incremental.rs`、`CHANGELOG.md`、`README*.md`、`scripts/release/build-deb.sh`、`scripts/windows/{conpty/fetch-conpty.ps1,package.ps1}`）。
- tag `v0.2.2` = `ddfd285`；`v0.1.0`/`v0.2.0` 仍指向 `08b55b4`（顺序无法调整，已裁定维持）。
- release：<https://github.com/nivrrex/leantty/releases/tag/v0.2.2>，说明取自 `docs/RELEASE-0.2.2.md`，3 个资产：
  `leantty_0.2.2_amd64.deb`（3,518,280 B / `dc8404ae…`）、`leantty-0.2.2-windows-x86_64.zip`（5,795,070 B / `d3ae8a3d…`）、`SHA256SUMS`（189 B / `870980b9…`）。
- 回下载校验：deb 与 zip 的 sha256 与本地资产目录**逐字节一致**（GitHub 侧 digest 亦相同）。
- 发布用 token 只经掩码输入注入，未落盘；发布结束后删除本地临时克隆（其 remote 含 token）。
- 已知未修：deb `Maintainer` 仍为占位符 `leantty <noreply@example.invalid>`（未获用户提供的真实联系地址）。
