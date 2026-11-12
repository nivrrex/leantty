# PHASE3A-LINUX：脏行增量渲染 / 启动分段耗时 / 共享测试与样例字体指引

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。

## 0. 合并与基线

- 起点 main @ b227835（phase2d-done）。本轮 `git fetch ali` 发现 ali/win 有新提交 `33287ab`/`41dc491`（2D-win：merge phase2d-done、2C 复验、pwsh 矩阵、启动计时），**按规则只记录、未合并**。
- 本轮所有 tag 均打在"test/clippy/run_all 全过"的提交上（phase3a-t2 之后的教训见 §5 偏差）。

## 1. 完成矩阵

| 任务 | 交付内容 | 验收证据 |
|---|---|---|
| 1 共享测试与样例 | `config` 的 `path_priority_cli_env_default` 按 `cfg(windows)`/`cfg(not(windows))` 分别用 APPDATA / XDG_CONFIG_HOME 构造期望，路径后缀改 Path 组件比较（仅改测试）；examples/linux.toml 与 examples/windows.toml 的 `[font]` 段补中文注释（主字体用小体积等宽西文、大体积 CJK 放 fallback 并说明同步加载成本）；windows 样例主字体 Microsoft YaHei Mono → Consolas，fallback 保持 Microsoft YaHei | Linux `cargo test -p config` 10 通过；**交叉编译** `cargo check -p config --target x86_64-pc-windows-gnu --tests` 通过（target 已安装）；样例断言同步更新 |
| 2 启动分段耗时 | `[perf] fontdb_ms` / `[perf] font_ms=<组>(family/style/used_style/cached/ms)` / `[perf] window_ms` / `[perf] surface_ms`（相对启动锚点 `render::mark_start()`，沿用既有 [perf] 机制，无新开关）；**缺陷修复**：四组主字体指向同一 fontdb face 时按 `face id` 缓存复用（原各自 from_bytes） | §2 实测表；同 face 场景 miss=1/hit=3，首帧 median 162ms vs 四文件 192ms（省 ≈30ms） |
| 3 脏行增量渲染 | FrameSnapshot 增 `dirty_rows`/`full_redraw`（owned，trait 未变）；VtTerminal 持久化 RenderState 使脏跟踪生效；`snapshot_incremental()` 收集脏行并 clean；render 抽出 `draw_row`（全量/增量共用）；叠加层正确性（选区旧/新行、光标旧/新行、Preedit 旧/新布局行、滚动条只重绘右缘 4px 条带、视口滚动/结构变化整屏）；present 只拷贝脏区并 `present_with_damage`，无脏区跳过 present | §3 正确性（单测对拍 + 2000 步随机序列 + e2e 截图 AE=0）；§4 性能表 |

## 2. 启动分段耗时（release 空闲，5 次）

| 配置 | fontdb_ms | 四组字体合计 | window_ms | surface_ms | first_frame_ms | bg_fonts_ms（CJK 到位） |
|---|---|---|---|---|---|---|
| examples/linux.toml（Consolas 缺失→Monospace，四组同 face） | 80.6–89.6（since_start 96.5–129.0） | 18.7–25.5（3 次 cached=true） | 0.5–1.9 | 0.3–1.8 | **150–183** | 634–690 |
| 同 face 显式配置（DejaVu Regular × 4 组） | 79.0–103.7 | miss=1/hit=3 | — | — | **160–179（median 162）** | — |
| 四不同文件（DejaVu Regular/Bold/Italic/BoldItalic） | 79.0–88.5 | miss=4/hit=0 | — | — | **184–215（median 192）** | — |
| 主字体名不存在（NoSuchFontXYZ → Monospace 回落） | 77.1–84.9 | miss=4/hit=0 | — | — | **192–209** | — |

结论（已验证事实）：
- Linux 首帧中 **fontdb 系统字体库加载约占一半以上（~85ms）**，四组字体解析 ~20ms，窗口+surface <2ms。
- face 缓存修复有效：同 face 复用省 ≈30ms（3 次 from_bytes 22ms → 3 次 Font::clone 4.6ms）。`Font::clone` 本身约 4–5ms/次，未进一步优化（记录）。
- 「缺失回落」不会退化，但四组各自解析一个文件（miss=4）。另外 `Consolas` 在 Linux 缺失时四组会统一回落到 Monospace Regular——**粗体/斜体不再有独立字形**，这是配置问题，样例注释已提示改用存在的小字体。
- Windows 侧首帧 1465ms 的原因**未定位**（待验证）：本日志为平台无关代码，Windows 用同一二进制跑即可对比三阶段数值。

## 3. 脏行语义（绑定确认）

- `Snapshot::dirty()` → `Dirty{Clean, Partial, Full}`；`RowIterator::next_dirty()` 在 Partial 时逐行给出（y=视口行号）；`Snapshot::clean()` 清除全局与行级标记（文档要求"完整帧渲染成功后"调用）。
- 实测（`crates/vt/tests/dirty_probe.rs`）：初始 Full；feed 两行 → Partial 且脏行 `[0,1]`；无变化 → Clean（无脏行）；单格改动 → 脏行 `[1]`；resize → Full；输出导致滚动 → Full；视口滚动 → Full。
- **关键实现约束（已验证事实）**：脏跟踪依赖**同一 RenderState 跨 update 持久**。原 snapshot 每次 `RenderState::new()` → 探针实测每一步都是 Full。改为 VtTerminal 内 `RefCell<Option<Box<RenderState>>>` 复用后语义恢复。
- **光标移动不产生行脏**（已验证事实）：`feed("\x1b[2;1H")` 后脏行集合为空 → 光标/选区/Preedit/滚动条等叠加层脏必须由渲染侧/app 侧补齐。
- API 拆分：「`snapshot_incremental()`（收集+clean，仅渲染路径每帧一次）」与「`snapshot()`（不收集不清除）」并存——因为复制选区文本、双击选词、IME 光标区、滚动条几何等会额外调用 snapshot，若在其中 clean 会吞掉本帧脏信息导致漏绘。

## 4. 增量渲染正确性与性能

### 4.1 正确性（三条独立证据）

1. **定向序列单测**（`crates/render/tests/incremental.rs::incremental_matches_full_pixel_exact`）：逐行修改、光标移动/隐藏/四样式、选区四态（含清空）、滚动条五态（含出现/消失）、Preedit 四态、反显翻转、宽字符——每一步增量帧缓冲与强制全量**逐像素相等**。
2. **随机序列**（`incremental_random_sequence_2000_steps`）：固定种子 **20260201**（可用 `APP_RANDOM_SEED` 覆盖），2000 步混合操作（写文本/清屏/滚动/resize/缩放/选区/光标/preedit + 滚动条随机），每步对拍；失败打印种子与首个差异坐标。**实测通过**（约 22s）。开发中该测试确实抓到两个真实缺陷：光标行被无条件标脏（导致滚动条帧总有行重绘）、Preedit 仅在"行/列/单元数"变化时才标脏（文本内容变化漏绘，已在键中加入 FNV 文本哈希）。
3. **e2e case26**：同一输入序列分别以增量与 `APP_FORCE_FULL_REDRAW=1`（debug 专有）运行，`import -window` 截窗口 + `compare -metric AE` = **0**。为消除 Xvfb 滚轮事件偶发丢失造成的非确定性，case26 不含滚轮步骤（视口滚动路径由随机序列覆盖）。

### 4.2 性能（release，预热 20 帧 / 采样 200 帧，中位数）

| 配置 | 全帧 | 1 行变化 | 1 单元格 | 光标移动 | 仅滚动条 | 满屏滚动输出 |
|---|---|---|---|---|---|---|
| 120x30@1x | 6.58ms | **0.21ms（1/31）** | 0.22ms | 0.43ms | 0.00ms | 6.52ms |
| 120x30@2x | 21.99ms | **0.69ms（1/32）** | 0.70ms | 1.38ms | 0.01ms | 20.91ms |
| 200x60@1x | 23.22ms | **0.36ms（1/64）** | 0.37ms | 0.73ms | 0.01ms | 22.29ms |
| 200x60@2x | 72.91ms | **1.16ms（1/63）** | 1.16ms | 2.30ms | 0.01ms | 70.74ms |

- 达成并超过"1 行变化 ≤ 全帧 1/8"目标（实际 1/31–1/64）。1 单元格与 1 行耗时基本相同（行级重绘粒度）。
- 满屏滚动输出 ≈ 全帧（全部行脏，符合预期）；此场景是 CPU 路径的主要成本来源。
- 冷/热差异很小（120x30@1x 冷首帧 6.99ms vs 热 6.60ms），说明静态画面成本主要是像素填充而非字形光栅化。

### 4.3 present 提交路径（Xvfb 实测）

softbuffer 0.4.8 提供 `present_with_damage(&[Rect])`（X11+XShm / Wayland / Win32 生效，其他后端等价 present）。

| 路径 | present 中位 | p95 |
|---|---|---|
| 脏区拷贝 + present_with_damage | **54–59µs** | 65–139µs |
| 全缓冲拷贝 + present（`APP_PRESENT_FULL=1`） | **76–84ms** | 88–99ms |

Xvfb 无共享内存时全缓冲提交慢约 1000 倍（25s 内仅完成约 50 帧）。**真实 X11（MIT-SHM）与 Windows 需真机复测**（登记为待验证项）。

## 5. 未解决问题与后续优化点

- **视口滚动仍整屏重绘**：未做帧缓冲位移复用（scroll blit）。这是 CPU 路径下一个最大优化点（滚动输出场景 ≈ 全帧成本）。
- **单元格级脏区未做**：当前行级重绘，1 单元格与 1 行同价（0.21ms @120x30@1x）。若要进一步降低小改动成本，需要单元格/矩形级脏区 + 对应 present damage。
- **`Font::clone` 成本 4–5ms/次**：face 缓存命中时仍需克隆字形表；若四组共享同一字体，可改为 `Arc<Font>` 共享（需改 groups 类型，属后续优化）。
- **scrollback 截断后的选区重定位**（2C 遗留）仍未做内容锚点重定位。
- **Windows 首帧 1465ms 未定位**：需 Windows 用本轮日志取数（见 §6）。
- **全缓冲 present 在 Xvfb 下 76–84ms** 属环境特性，真实 X11 需复测；若真机也慢，应确保永不回退到全缓冲路径。
- 备注：`phase3a-t2` 打在了 47d3886，而该次 `run_all` 有 1 例 FAIL（其后安全修复提交复跑已 78/78）；严格按"tag 必须打在验收全过的提交上"，本轮后续 tag 均在提交前完成 3 连验收（t3 起已执行）。

## 6. 给 Windows 侧的交接

1. **需复验项**：增量渲染在 Windows 下的行为（ConPTY 输出 → 行脏 → 增量绘制）；`present_with_damage` 在 Win32 后端（softbuffer 支持）是否生效、与全缓冲的耗时差；`APP_FORCE_FULL_REDRAW=1` 截图对拍（e2e case26 同法，需 Windows 侧截图工具）。
2. **需取数项**：用本轮新增的 `[perf] fontdb_ms / font_ms / window_ms / surface_ms / first_frame_ms / bg_fonts_ms` 在 Windows 上跑默认配置与 Consolas+YaHei fallback 配置各 5 次，定位 1465ms 的构成（fontdb？字体解析？present？），据此决定是否实施 2D 评估中的主字体延后加载（方案 a）。
3. **新增 debug 开关**（release 不含）：`APP_FORCE_FULL_REDRAW`（强制整屏重绘）、`APP_PRESENT_FULL`（强制全缓冲 present）、`APP_PRESENT_STATS`（每 20 帧打印 present 耗时中位/p95）；`APP_RANDOM_SEED` 可覆盖增量随机序列种子。
4. Windows 专属文件本轮未编辑（`crates/pty/src/windows.rs`、`pty_windows.rs`、`scripts/windows/`、`docs/WIN-NOTES.md`、`vt-verify-win/`、`.cargo/config.toml` windows-gnu 段）；ali/win 的 `33287ab`/`41dc491` 未合并。

## 7. 手动清单（≤3）

1. 真实 X11（MIT-SHM）与 Windows 下的 present 两路径耗时复测（Xvfb 数据不代表真机）。
2. Windows 首帧分段耗时取数（定位 1465ms）。
3. 真实 HiDPI 屏幕下的增量渲染观感（残影/闪烁）确认。

## 8. 验收

- `cargo test --workspace`：**93 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：0
- `cargo build --release`：通过
- `scripts/e2e/run_all.sh`：**79/79 PASS，连续 3 次全过**
