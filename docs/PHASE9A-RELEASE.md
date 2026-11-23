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

## v0.2.3 发布（9D：IME 候选窗定位）

- 分支 `rel-0.2.3`：`a966db7`（修复）→ `66df59e`（版本 0.2.3 + 发布说明 + deb changelog 条目更新）。
- 交付物：`leantty_0.2.3_amd64.deb` 3,522,972 B / `58a3c4e5…`（包内 `/usr/bin/leantty` = `815f6c65…`，`--version` = `leantty 0.2.3`）；
  `leantty-0.2.3-windows-x86_64.zip` 5,799,702 B / `992d0b2c…`（10 条目，`leantty.exe` 12,812,800 B）。
- Windows 二进制**在 Linux 上交叉编译**（`cargo rustc --release --target x86_64-pc-windows-gnu` + `x86_64-w64-mingw32-windres` 注入图标资源），
  已验：PE `i386:x86-64`、`.rsrc` 段存在（0x1ac78）、字符串含 `leantty 0.2.3`；ConPTY 对取自 0.2.2 归档并校验 sha256 与脚本内固定值一致。
  **未在 Windows 实机运行**（发布说明中已标注）。
- GitHub：公开仓库提交 `a75cf99`「leantty 0.2.3」、tag `v0.2.3`、release id `451143897`
  （<https://github.com/nivrrex/leantty/releases/tag/v0.2.3>），3 资产回下载校验与本地一致、`sha256sum -c` 通过。
- 未打 phase tag：`scripts/tag_checked.sh` 要求 `gates_ok == HEAD`，而全量 gates 含 Linux e2e（约 25 分钟），本次按"只测受影响范围"的要求未运行；
  如需 phase tag 需先跑一次全量 gates。

## v0.2.3 覆盖发布（含 9E：WM 组合键/失焦修饰键修复）

- 代码：`faf5905`（WM 组合键不写 PTY、失焦清修饰键、无焦点丢弃按键）+ `8d42a27`（R4 评估留档，未改）；
  版本号保持 **0.2.3**，CHANGELOG 的 9E 条目并入 0.2.3 段。
- 重新构建：deb `leantty_0.2.3_amd64.deb` **3,522,352 B** / `11dc05ac…`；
  Windows zip **5,800,053 B** / `c703db96…`（10 条目，`leantty.exe` 12,813,312 B，`.rsrc` 图标资源已验，
  版本串 `leantty 0.2.3`；ConPTY 对 sha256 仍与 `package-release.ps1` 固定值一致）。
- GitHub：公开仓库新提交 `2cba2ce`「leantty 0.2.3 (repack: Alt+Tab / Super 组合键修复)」；
  release `451143897` 的 3 个资产已删除并重新上传，回下载 `sha256sum -c` 通过。
- 说明：tag `v0.2.3` 仍指向上一版提交 `a75cf99`（不改写 tag，规则禁止 force/移动 tag），
  因此该 tag 的源码快照不含 9E；release 资产以本记录与 `SHA256SUMS` 为准。

## 0.2.3 重新发布 + 0.2.4 发布（2026-11-20）

- **v0.2.3 标签重指**：`v0.2.3` 之前指向 `a75cf99`（重打包前的源码），与资产不一致。本次删除远端 tag 后在
  `2cba2ce`（"leantty 0.2.3 (repack)"）上重建并 push（不是 force push，但确实改写了 tag 指向，用户已明确要求）。
- **注意**：删除 tag 会连带删除 GitHub 上绑定的 release（实测 0.2.3 的 release 与资产随之消失），
  重建后新建 release（id `452247836`）并重传资产；新资产上传后即刻匿名下载会 404（CDN 传播延迟），
  等约 25 s 后恢复 200。
- **v0.2.4**：源码内容与重打包后的 0.2.3 相同，仅版本号 +1，目的是让 tag/源码/包严格对应。
  deb `leantty_0.2.4_amd64.deb` 3,519,172 B / `d78d5723…`；zip 5,800,176 B / `c60b00e9…`（10 条目）；
  release id `452247024`，tag `v0.2.4` = `47b46d0`。
- 两个 release 的 3 个资产均回下载 `sha256sum -c` 通过；release 列表：v0.2.4 / v0.2.3 / v0.2.2 / v0.2.0 / v0.1.0，各 3 资产。

## 0.2.4 重打包（9E-3：焦点补投 Tab 修复）

- 取证：用户机器 `LEANTTY_KEY_TRACE` 记录显示 openbox 抓走 `Alt+Tab` 后，客户端只收到 `Tab Released`，
  随后 `Focused(true)` 之后 **0–13 ms 内补投一次裸 `Tab Pressed`**（mods 全空）——残留 Tab 的真实来源。
- 修复 `6ee542c`：`tab_is_focus_replay` 丢弃"焦点恢复后 ≤100 ms 的裸 Tab"（与 `tab_is_alt_tab_tail` 并用）。
- 新资产：deb **3,523,868 B** / `cc5373b3…`；zip **5,803,695 B** / `9c32993f…`；release id `452326017`，
  tag `v0.2.4` = `c85b67d`；回下载 `sha256sum -c` 通过。
- 再次踩到同一个坑：删除/重指 tag 会连带删除该 tag 的 release（这次是 v0.2.4），需重建 release 并重传资产。

## v0.2.5 发布（9F：字号对齐 + 三类丢键修复）

- 依据 `leantty-zig/RUST-ISSUES.md` §4 的按键对拍结论，逐条核实后修复：
  - `Ctrl+Space` 无输出 → `Named(Space)` 未映射到 `vt::Key`，编码分支整体跳过；补映射后发 `0x00`。
  - `Ctrl+Shift+<字母/符号>` 无输出或发 `CSI u` → 编码器第 5 参数 `utf8` 恒为 `None`，且移位字符（`{`/`}`/`|`）
    映射不到按键枚举。改为把 `event.text` 交给编码器，并在**未启用 Kitty 协议**时把 `CSI u` 回写成
    xterm 控制字节（新增 `ctrl_byte` / `is_csi_u`，`VtTerminal::kitty_keyboard_enabled`）。
    探针实测：`Ctrl+Space→00`、`Ctrl+Shift+A→01`、`Ctrl+Shift+[→1b`、`Ctrl+A→01`、`Ctrl+Shift+Z→1a`。
- 字号对齐 alacritty：`font.size` 改按点解释（`px = pt × scale × 96/72`，新增 `px_from_pt`），
  格宽由 `ceil` 改 `round`。对照实测（80×24、padding 0、DejaVu Sans Mono 16）：两程序窗口均为 **1520×912**
  （改前 leantty 1600×912）。
- 资产：deb **3,519,740 B** / `411b7512…`；zip **5,804,355 B** / `5e2f2604…`；release id `452344379`、
  tag `v0.2.5` = `e2d37d4`；回下载 `sha256sum -c` 通过。0.2.4 的 release 说明已重写（不再是"与 0.2.3 功能相同"）。

## v0.2.6（9H：桌面启动首行空行 + 格高对齐）

- 通过 SSH 在用户 office 机（Devuan 5、`:0`、Xft.dpi=144）实机定位：
  - **首行空行**：桌面会话无 `TERM` → 子进程被 bash 当 dumb 终端；窗口建成后的一次尺寸变化让它用空格重画提示符，
    把第 0 行擦空、提示符落到第 1 行（`first_frame cursor=(18,1) row0=""`）。PTY 字节追踪显示首块是
    `ESC[?2004h` + 提示符（无换行），随后两块是 `\r` + 大段空格 → 确认是 shell 的 dumb 重画，不是我们多写换行。
    修复：`TERM` 缺失或 `dumb` 时给子进程兜底 `xterm-256color`（`crates/pty/src/unix.rs`）；修复后 `cursor=(18,0)`、`row0` 即提示符。
  - **格高多 1 px**：`cell_metrics` 行高由 `ceil` 改 `round`（与 alacritty 同口径）。实机对照：0.2.5 窗口 1680×930（格 14×31），
    alacritty 1680×900（格 14×30）；修复后 leantty 也是 1680×900。
- 资产：deb **3,529,976 B** / `aba26e58…`；zip **5,811,470 B** / `1d3f8923…`；tag `v0.2.6` = `91f1aec`；
  回下载 `sha256sum -c` 通过。远端临时诊断二进制与 `/tmp/off*` 已清理，用户机器上的 desktop 文件已改回 `Exec=leantty`。

## v0.2.7（9I：下划线样式 + 符号字体兜底）

- 逐项核对 `leantty-zig/RUST-AGENT-TASKS.md`（对照 v0.2.4 的清单）：
  - **T1 成立但不完全**：`Double/Dotted/Dashed` 其实已有，只是点线/虚线用"每 N 像素点一次"且**Curly 退化成实线**。
    重写为：线宽 `max(h/16,1)`、基线 `h-(线宽+max(h/12,1))`，Double 两条独立线、Curly 每 2 px 上下交替、
    Dotted 1 亮 1 灭、Dashed 2 亮 2 灭。新增 `crates/render/tests/underline_styles.rs` 锁死（回退成实线即变红）。
  - **T2 成立**：回退链只来自用户配置。新增内置兜底 `Symbola` / `DejaVu Sans` / `Noto Sans Symbols2`（追加在用户链之后，
    缺失自动跳过）；实测 `😀 🚀 🦄` 由豆腐块变为单色符号。彩色 emoji（CBDT）仍不支持，已记入发布说明。
  - **T3 已在 0.2.5/0.2.6 解决**：其测量用的是 v0.2.4（`font.size` 当时还当像素用）。在用户 office 机上用同一份配置实测
    0.2.7 与 alacritty 窗口均为 **1680×900**，Zig 清单里的"小 20%"不再成立。
- 交付：deb **3,525,412 B** / `cefe8e2c…`；zip **5,812,483 B** / `304855d0…`；tag `v0.2.7` = `dcccc14`；
  release id `452472552`；回下载 `sha256sum -c` 通过。

## v0.2.9（9K：字形覆盖改真实 cmap）

- 根因（已验证事实）：crossfont 的 `get_glyph` 在缺字时**不报错**，会通过 fontconfig 换 face 渲染并返回 Ok
  （`crossfont/src/ft/mod.rs::face_for_glyph`）；因此 0.2.8 用 `is_ok()` 判"face 是否含该字"无效 →
  `index_for` 恒为 0、配置的 `font.fallback` 被绕过、逐字符字形由 fontconfig 自选（字重/字号不一致，
  即用户看到的"部分文字渲染问题"）。已用测试固化该事实（`font_metric_chain.rs`）。
- 修复：`FontSet` 记录每字体的 family/style，覆盖率按 fontdb（家族→文件）+ ttf-parser（cmap）查询，
  **懒加载**（仅主字体缺字时才扫系统字体，纯 ASCII 启动零开销）；家族名要求精确匹配，避免落到替代字体。
- 验证：新增 `cjk_resolves_to_configured_fallback`（A→主字体、中→fallback、链外字符→None）；
  `wide_glyph` 用例改为显式给出含汉字的回退链；gates 10 步全过、e2e 131/131、GPU 轮 10/10。
- 交付：deb 3,519,892 B / `e09b3d0e…`；zip 5,757,716 B / `16a400f1…`；tag `v0.2.9` = `58ba899`；
  release id `453826479`；回下载 `sha256sum -c` 通过；phase tag `phase9j-done` = `ee7c7dc`。
- 未解决：Windows **最大化偶发崩溃**——代码审查未找到确定崩溃点（0 尺寸、NonZero、surface.resize 失败、
  fill_rect 边界、atlas 上限均有处理），需用户提供崩溃时的 stderr（Rust panic 会打印）或 WER 记录。

## v0.2.10（9L：GPU present 可观测性 + 遮挡跳过）

- 核实 Zig 侧三条结论：
  - **auto 过滤软适配器：成立（无需改）**。`Gpu::new` 跳过 `DeviceType::Cpu`（打印 `[perf] gpu_skip_software`），
    且 `auto` 永远忽略 `LEANTTY_GPU_ALLOW_SOFTWARE`。Xvfb 上 `auto` 选到真实 `NVIDIA GeForce GT 710`——
    Vulkan 在 Xvfb 下仍枚举真实设备，不是过滤失效。
  - **GPU 的 `present_ms` 恒为 0：成立（已修）**。GPU 路径此前固定 `pf.present_us.push(0)`，perf 行看不到
    present 阻塞，只会让 `render_ms` 变大。现在 `get_current_texture`（acquire）与 `queue.present` 分别计时，
    perf 行新增 `present_ms_max`。实测：修复前 `present_ms(med=0 p95=0)`，修复后 `med=19 p95=21 present_ms_max=21`。
  - **遮挡时仍在 present：成立（已修）**。此前完全没处理 `WindowEvent::Occluded`；FIFO 下窗口被完全遮挡时
    合成器不消费帧，`get_current_texture` 可长时间阻塞。现在遮挡期间跳过渲染/present（PTY 照常喂入），
    恢复可见时补一帧，与 alacritty `draw()` 的 `occluded` 早退一致。
- 未改：present 模式仍为 `Fifo`（alacritty 默认），是否换 `AutoNoVsync` 需先拿真机 `present_ms_max` 数据。
- 验证：`cargo test --workspace` 42 个测试二进制全绿、clippy（含 Windows 目标）0 告警、gates 10 步全过、
  e2e 131/131、GPU 轮 10/10；Xvfb + 真机 GPU 上 30 MB 输出遮挡/非遮挡两组帧率与 coalesced 正常。
- 交付：deb 3,520,224 B / `6d7908b8…`；zip 5,758,100 B / `b2c34713…`；tag `v0.2.10` = `0b2e2f3`；
  release id `454092295`；phase tag `phase9k-done` = `d7bea3d`；回下载 `sha256sum -c` 通过。
- 限制：X11 下 winit 不投递 `Occluded`（实测遮挡窗口后仍出帧），该修复主要面向 Windows/Wayland；
  真实桌面 `:0` 与 Windows 实机均未验证。

## v0.2.11（9L+：合并 Windows 字形回退链）

- 合并 `win` 分支（`f3d949c` 起 6 个提交）：Windows 未配置 `font.fallback` 时播种
  `Segoe UI Symbol → Segoe UI Emoji → Microsoft YaHei → MS Gothic`；风格名非 "Regular" 的 face
  （如 DejaVu 的 `Book`）在 DirectWrite 下退化为按描述加载；空格等无位图字符仍画下划线/删除线。
- Linux 侧行为不变（仍为配置链 + `Symbola`/`DejaVu Sans`/`Noto Sans Symbols2`），由
  `crates/render/tests/font_fallback_default.rs` 的 5 项单测按平台断言。
- 验证：Linux `cargo test --workspace` 43 个测试二进制全绿、clippy 0 告警、gates 10 步 PASS
  （e2e 131/131、GPU 轮 10/10）；Windows 侧 `cargo test` 264 passed、`[cov]` 前后对比见
  `docs/WIN-SYMBOL-FALLBACK.md`。
- **门禁 flake 记录**：0.2.11 期间 e2e 三次跑出现两次零散 FAIL（`case26/27` 增量=全量像素差、
  `case22` 的 `ctrl+d` 被当字面 `d` 输入导致 app 不退出），均为 Xvfb 下输入时序抖动（dump 里能看到
  `dddd…` 被写进 shell），同一提交随后连跑 `run_all.sh` 两次 131/131、gates PASS。**不是代码回归**；
  若再现，先看失败 dump 里是否有字面修饰键字符。
- 交付：deb 3,518,632 B / `e5a12be6…`；zip 5,540,424 B / `867addd1…`；tag `v0.2.11` = `b06277b`
  （release `454407478`）；`phase9l-done` = `c5feebb`（门禁 hash）；回下载 `sha256sum -c` 通过。

## v0.2.12（9M：Windows 多行粘贴）

- 症状：Windows 上向开了 `?2004` 的应用（pi）粘贴 7 行，只进第一行并逐行提交；Zig 版同样。
  Windows 侧字节级取证：子进程收到 `ESC[200~`/`ESC[201~` 全丢，且每个 CR 变成 `0D 0D`。
- 修法：`paste::paste_newline(bracketed, windows)`——Windows + 括号粘贴用 LF，其余 CR；
  孤立 CR 在 LF 模式归一；两个标记都从正文过滤；包裹仍在分块之前（单测锁定只有一对标记）。
- **Linux 不影响**：单测按参数覆盖两平台，另有端到端探针 `scripts/e2e/paste_bracket_probe.sh`
  实测 7 行=103 B/CR7、370 KB=CR20000，均单对标记、无 CRCR、正文字节与源一致。
- conpty.dll 旁加载**已具备**（portable-pty 0.8.1 先 kernel32 再 `LoadLibrary("conpty.dll")`，
  程序目录优先），因此发布 zip 的 OpenConsole 会被使用；若实机仍丢标记，先查两文件是否同目录。
- 验证：`cargo test --workspace` 43 个测试二进制全绿；gates 10 步 PASS（e2e 131/131、GPU 10/10）。
  期间 e2e 再出现一次 `case22` 抖动（三跑两过），证据仍指向 Xvfb 输入时序，未改判据。
- 交付：deb 3,522,748 B / `382829d6…`；zip 5,541,503 B / `75bd6456…`；tag `v0.2.12` = `26d43af`
  （release `454717380`）；`phase9m-done` = `5c440b9`；回下载 `sha256sum -c` 通过。
- 未验证（需 Windows 实机）：粘贴进 pi 是否整块、cmd 是否仍逐行、实机 conhost 是否真丢标记。

## v0.2.13（9N/9O 合并：合成粗体）

- 缺陷（Linux 实测）：单字重字体（`汉仪旗黑 55S`，`fc-match` 的 Bold/Regular 同一文件）下
  `SGR 1` 与 `SGR 0` 渲染**逐字节相同**（墨迹 95064 = 95064，+0.0%），而 DejaVu 真 Bold 为 +51.9%。
  根因：族内无 Bold 面时 crossfont/FreeType 把两个请求解析到同一 face，不做加粗；alacritty 同样如此。
- 修法：`fontset.rs::needs_synthetic_bold()`（按"常规组与粗体组同索引 face 是否渲染出同一张位图"判断，
  覆盖"主字体有 Bold + 回退 CJK 无 Bold"）+ `embolden_bitmap()`（水平右向膨胀 `d=round(px/24)` 至少 1px，
  再叠加垂直分量：上一行 ×0.6，全部限制在 bbox 内不增高）。
- 实测：只做水平时拉丁 +39.6%、中文 +34.8%（真 Bold +51.9%/+52.8%）；补垂直后 **拉丁 +48.3%、
  中文 +48.2%**，贴近真 Bold。视觉对照 `/root/boldshot/compare.png`。
- 配置：`[font] synthetic_bold`（默认 true；false = alacritty 行为），两平台示例配置均带注释。
- 测试：`synthetic_bold.rs` 4 项 + `fontset::embolden_tests` 3 项 + config 3 条断言。
- 工具链（项目硬性规定）：`scripts/gates.sh` 第 1 步改为 `cargo nextest run --all` + `cargo test --doc`，
  nextest 缺失时回退并告警；250 个测试 5 分钟 → **56 秒**。
- 门禁期环境问题（非代码，已排除）：残留 `:99` Xvfb 会让下一轮 e2e 大面积"窗口未出现"（跑门前清
  `pgrep -f "^Xvfb :99"` 与 `/tmp/.X99-lock`）；e2e 中途 X 掉线两次重跑即过；`case22` 的 `ctrl+d` 偶发被当字面 `d`；
  `LEANTTY_KEY_TRACE=1` 会生成名为 `1` 的追踪文件（该变量取路径）。
- 交付：deb 3,521,920 B / `c47f2ae2…`；zip 5,543,809 B / `93ab46f6…`；tag `v0.2.13`；`phase9o-done` = `4bdf083`。
- **版本合并**：0.2.13 与 0.2.14 合并为单独的 0.2.13 发布（0.2.14 未单独发布过，其 release 与 tag 一并撤掉）。
