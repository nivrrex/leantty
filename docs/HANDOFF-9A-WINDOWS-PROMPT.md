# 给 Windows Agent 的提示词（直接复制粘贴使用）

> 用法：把下面「提示词正文」整段发给在 Windows 机器上工作的 agent。它在此之前应先读过项目根目录的 `AGENTS.md`。

---

## 提示词正文

你要在 **Windows 机器**上接手 leantty 的 9A 收尾，只做 Windows 侧的事，**不要碰 Linux 侧的判断和阈值**。

### 背景（已由 Linux 侧完成，不要重做）
Linux 侧在 `rel-0.2.2` 分支修完两个缺陷并打好了 tag，等你把改动合并到 Windows 分支、验证、重打 zip：

1. `phase9a-done2`（`5675ac7`）——修 **GPU 后端中文显示为方块**：
   - 根因 A：`GpuRenderer::add_background_fonts` 只清 `char_font`，未清 glyph atlas；atlas 键是
     `(group, ch, px, scale)` 不含字体索引，并入前按 notdef 存的槽位在并入后被同键命中。
   - 根因 B：若用 `GlyphAtlas::reset()` 清空会新建 texture/view，而渲染绑定组缓存旧 view，
     所有字形采样空纹理。最终实现是新增 `GlyphAtlas::clear(queue)`：只清映射与像素、整张重传、
     **保留同一张纹理**。
   - 验证：同配置下 CPU 与 GPU 就绪后截图逐像素一致（差 0）；`case81` 增强为三项断言。
2. `phase9a-perf`（`b785881`）——修 **启动早期 ~0.65 s 方块**：回退字体加载提前到 `resumed` 建窗前发起，
   并通过新增的 `load_font_list_shared` / `add_background_fonts_shared` 省掉一次 CJK 深拷贝
   （每面 ≈137 ms）。Linux 实测 0.65 s → **0.31 s**；剩余时间受单面 CJK 解析（约 0.3 s）限制，
   **已作为遗留问题记录在 README 与 DECISIONS，你不要试图继续优化它**。

### 你的任务（按顺序，全部在 Windows 分支上）
1. **合并**：从 Windows 当前分支（`phase8c-win-done` 或其后续）拉新分支，依次合并
   `phase9a-done2`、`phase9a-perf`。预期冲突只有 `docs/DECISIONS.md`（双方各自追加，**两段都保留**）。
   `crates/` 若与 Windows 本地改动重叠，按"两边都要"合并，不要丢弃任何一侧。
2. **核对合并范围**：`git diff --stat phase8c-win-done..HEAD -- crates/` 应只涉及
   `crates/render/src/gpu/atlas.rs`、`crates/render/src/gpu/mod.rs`、`crates/render/src/lib.rs`、
   `crates/app/src/lib.rs`。多出来的文件要能逐条解释，解释不了的停下来报告。
3. **编译与检查**（在项目声明的目录执行，命令与 8D 一致）：
   - `cargo test --workspace`
   - `cargo clippy --all-targets -- -D warnings`（debug 与 release 各一次）
   - `cargo check --target x86_64-pc-windows-gnu --workspace --tests`
   - 若有 Windows 专属构建脚本/CI，按其约定跑
4. **三项运行时验证**（这是本次的核心，缺一项就不能宣称完成）：
   - **A. GPU 字形**：`backend = "gpu"` + 配置 CJK fallback（`HYQiHei` 或 `Microsoft YaHei`），
     冷启动后中文提示符必须是**真实字形**；截 GPU 与 `backend = "cpu"` 两张同尺寸截图，逐像素比对，
     差异应只落在字形光栅化（与既有"约 1% 像素不同"的已知限制一致），**不能出现整字消失或方块**。
   - **B. 字体并入后必须重绘**：记录 `bg_fonts_ms` 与 `first_frame_ms`（`LEANTTY_PERF_FRAMES=1`），
     确认并入后画面从方块变成中文；Windows 的字体扫描更慢，**方块期 >0.5 s 属预期，记录实际值即可，不要去调阈值**。
   - **C. 回归**：老的"3 秒才自动恢复正常"症状应消失；用与 `case81` 相同的逻辑（CPU/GPU 就绪后像素一致）确认。
5. **重打 zip**：`cargo build --release --target x86_64-pc-windows-gnu`，按 8D 同款内容打包
   `leantty-0.2.2-windows-x86_64.zip`，解压后应仍为 6 项（`leantty.exe`、`config.toml`、
   `README.md`、`README.zh-CN.md`、`LICENSE`、`THIRD-PARTY-NOTICES.md`）。

### 约束（与 Linux 侧同一套，违反即返工）
- **不要**把 `main` 合进你的分支（main 含未发布的 8B 观测改造）。
- 不要改任何阈值、基线、判据；不要"顺手"优化启动方块期（已裁定为遗留问题）。
- 不要改 `Renderer` / `Pty` 的既有签名；需要扩展时只新增入口并让旧入口委托。
- 不要新增配置项、依赖，不要改 `vendor/`。
- 不 force push、不移动已有 tag；新 tag 必须经 `scripts/tag_checked.sh`（工作区干净、`gates_ok` hash == HEAD、
  名字匹配 `^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$`）。
- 未实机验证的东西要明确标"未验证"，不得写成结论。

### 交付格式（照这个回）
```
合并后 HEAD: <sha>
合并提交: <sha> <标题>
冲突文件: <列表，与原因>
测试与检查: <命令 → 结论，逐条>
case81 / 三项运行时验证: PASS|FAIL（附 CPU vs GPU 像素差、bg_fonts_ms、first_frame_ms）
zip: leantty-0.2.2-windows-x86_64.zip  <字节数>  <sha256>
exe: <sha256>
未验证项 / 已知限制: <列表>
```
