# PHASE2A-LINUX：配置文件系统 / 字体回退链 / 度量网格 / HiDPI / 渲染选项 / debug 钩子

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。

## 1. 完成项（全部自动验收通过）

- 任务1 config crate：TOML、字段名与 alacritty 对齐 + 新增 font.fallback；未知字段带路径警告；类型/颜色/数值越界字段级回落默认不 panic；路径优先级 --config > APP_CONFIG > 默认路径（XDG/APPDATA + 二进制名）；解析与应用解耦（config 不依赖 winit/render/vt）；examples/linux.toml + examples/windows.toml 无警告加载单测。
- 任务2 度量网格 + HiDPI：删固定 8x16；render::cell_metrics 纯函数（宽 '0' advance、高 fontdue LineMetrics::new_line_size，ceil 取整）；window.dimensions 决定初始行列，窗口逻辑尺寸 = 行列*格子 + 2*padding；resumed 取真实 scale_factor 修正；WindowEvent::ScaleFactorChanged 经 InnerSizeWriter 保持 dimensions；Resized 反算行列 min(1) 兜底；e2e case7 动态双侧一致（640x480 → cols=49 rows=22 由度量算出）+ case9 120x30 双侧一致。
- 任务3 回退链与渲染选项：四组字体 × [主, fallback..., Monospace, SansSerif] 链；style 缺失回落 Bold→Italic→Regular（日志，不合成）；逐字符 has_glyph 回退 + 码点缓存；字形位图缓存（键含组/码点/px/选项指纹/gamma，scale 变化整体失效）；宽字符 2 格 fit 缩放（不放大）+ 居中 + ymin 基线对齐；SpacerTail 不绘制（对照帧断言）；样式（反显/faint/underline 5 种/删除线/上划线/粗体亮色）与光标四样式渲染；渲染选项 antialiasing/subpixel/text_gamma/draw_bold_with_bright 实现，hinting 不支持（unsupported 日志）；颜色表 config→render 自有 Palette 转换。
- 任务4 钩子门控：APP_DUMP_SNAPSHOT 全钩子 #[cfg(debug_assertions)] 门控（函数+调用点+字符串）；e2e 用 debug 二进制；run_all 内置 release strings 检查（PASS release-hook-gated）；DECISIONS 登记长期保留仅 debug。
- 验收：cargo test --workspace 33 passed / 0 failed；clippy --all-targets -D warnings 0；build --release 通过；run_all 26/26 PASS；examples/linux.toml 启动截图 scripts/e2e/out/phase2a-linux-config.png（窗口内背景采样 #F3F1E9 = 配置 primary.background 原值，dump cols=120 rows=30，命令回显正常）。

## 2. 配置字段支持矩阵（字段 → 状态）

| 字段 | 状态 |
|---|---|
| window.position | 仅解析（winit 初始位置未接线） |
| window.dimensions | 已生效（初始行列+窗口尺寸） |
| window.padding | 已生效（窗口像素 = 行列*格子 + 2*padding） |
| window.dynamic_title | 仅解析（标题固定 "tty"） |
| window.opacity | 仅解析保留（渲染未实现，DECISIONS 登记） |
| scrolling.history / multiplier | 仅解析（滚动未实现，2B 范围外） |
| terminal.shell.program | 已生效（spawn） |
| terminal.shell.args | 仅解析（Pty::spawn trait 不含 args，改 trait 属停止条件） |
| font.size / normal / bold / italic / bold_italic | 已生效 |
| font.fallback | 已生效（有序回退链） |
| font.antialiasing / subpixel / text_gamma / draw_bold_text_with_bright_colors | 已生效 |
| font.hinting | 仅解析（fontdue 不支持，new() 打印 unsupported 日志） |
| selection.* | 仅解析（选区 2B） |
| colors.*（primary/cursor/normal/bright/draw_bold） | 已生效 |
| mouse.bindings / keyboard.bindings | 已解析+表单测（动作执行 2B） |

## 3. 字体回退规则与缩放规则（已验证事实）

- style 查找：family + style 精确 → 缺失回落 Bold→Italic→Regular（日志，不合成粗体/斜体）。
- 逐字符回退：组内 [主字体, font.fallback..., 系统 Monospace generic, SansSerif] 取首个 has_glyph 码点的字体；码点→字体索引缓存（键 (组,码点)，与 scale 无关）；全无命中用组首字体（fontdue 缺字形返 default glyph，不 panic）。
- 宽字符缩放：Wide 格区域 = 2 格宽；位图缩放 s = min(2格宽/位图宽, 格高/位图高, 1.0)（只缩小不放大）；水平居中；垂直随缩放后 ymin 与基线对齐（fontdue 位图 ymin 即基线偏移，缩放均匀故基线自然对齐）；SpacerTail/SpacerHead 不绘制字形（背景仍填充）。
- 系统兜底与 fallback 跨组共享（from_bytes 一次性成本，DECISIONS 登记 6.1s 实测）。

## 4. 渲染选项实际状态

- antialiasing = grayscale（默认）/ none（二值化 >127→255）：已实现，像素差异有单测。
- subpixel = none（默认）/ rgb / bgr：已实现（fontdue 无亚像素 API，评估结论：3x 水平采样——px*3 光栅，每输出像素三通道取连续 3 列、垂直取中间行；垂直方向为近似，DECISIONS 登记）；未经批准未更换光栅化库。
- hinting：fontdue 不支持→只解析+new() 打印一次 unsupported 日志，不静默忽略。
- text_gamma（默认 1.0）：覆盖率^(1/gamma)→alpha 映射已实现，浅背景字细缓解路径就绪。
- draw_bold_text_with_bright_colors：粗体单元格 fg 命中 normal[i]→bright[i] 已实现。

## 5. 测试清单与结果（全部实测通过）

- 单元/集成（cargo test --workspace 33 passed）：config 10 项（examples 无警告加载/未知字段警告/颜色非法/数值越界/绑定表/mods/路径优先级）；render cell_metrics 缩放性、fallback_chain 3 项（ASCII 主字体/汉字回退索引、宽字符 2 格+SpacerTail 对照帧、Bold Italic 回落不 panic）、render_pixels 4 项（样式颜色/粗体亮色/光标四样式/选项像素差异）；app keyboard 3 项+reader 2 项；其余 crate 既有测试。
- e2e（run_all 26/26）：case1-8 既有+case7 动态双侧一致+case9 120x30 双侧一致+case10 examples/linux.toml 中文 printf 八进制回显+font 索引行+case5/6/9/10 no-wake 断言+release-hook-gated。

## 6. 未解决问题与偏差

- xdotool type 对 CJK 发 caret 记法文本（本环境不可用；中文 e2e 走 printf 八进制）；前 DECISIONS "xdotool type UTF-8 实测可用" 系误记，已更正登记。
- shell args / position / dynamic_title / opacity / scrolling / selection / 绑定动作：仅解析（范围或接口限制，DECISIONS/本文件登记）。
- fontdue from_bytes 对 CJK 字体全字形预建表，19MB 字体首启 6.1s（跨组共享优化后 e2e ~59s）；后续阶段可评估懒加载。
- subpixel 垂直方向单行采样为近似；Curly 下划线近似 Single；blink 常显。
- Xvfb 下 resumed 初始 request_inner_size 返回 None（窗口未接受 HiDPI 修正尺寸），已知噪音，ScaleFactorChanged 路径功能正常。
- 偏差：config 解析层手写 toml::Value（非 serde derive，字段级回落语义需要）；render 不依赖 config（自有 Palette/FontSettings 转换层）。

## 7. 给 Windows 侧的交接

- config 路径：default_config_path 已 cfg 分平台（Windows %APPDATA%\<app>\<app>.toml，<app> 取二进制名）；resolve_config_path 优先级 CLI > APP_CONFIG > 默认路径，跨平台同代码。
- examples/windows.toml：pwsh.exe（不硬编码绝对路径）+ Microsoft YaHei Mono + fallback Microsoft YaHei，启动方式 `--config examples/windows.toml`；config 单测含两 examples 无警告加载（跨平台编译）。
- 新增接口（Windows 侧对接）：config::{Config, parse, resolve_config_path, app_name, Error, Warnings}；render::{CpuRenderer::new(&FontSettings, RenderOptions, Palette), set_scale_factor, cell_metrics, font_indices, FontSettings, FontSpec, RenderOptions, Palette, Antialiasing, Subpixel}；app::App::new(config::Config)。
- Windows 注意：render 的系统兜底用 fontdb generic 链（Monospace→SansSerif）——Windows 字体枚举需 fontdb Windows 后端支持（本阶段未验证，待验证假设）；pty windows.rs 未改（win 分支合入时保留 pty_windows.rs 的 #![cfg(windows)] 门控豁免）；softbuffer 在 Windows 的 display handle 接线同 Linux 路径。

## 8. 用户本机手动确认清单（≤5 条，仅限 Xvfb 无法验证项）

1. 真实 HiDPI 屏幕下 ScaleFactorChanged 重排网格无闪烁、字形不模糊。
2. Consolas + 黑体回退链的真实观感（中西文混排基线对齐）。
3. 浅色主题（#F3F1E9 背景）长时间阅读观感与 text_gamma=1.0 字重。
4. 真实 GPU/显示器下亚像素渲染（subpixel=rgb）观感（Xvfb 为 CPU surface）。
5. window.opacity 未实现——如需透明度请确认是否进入后续阶段范围。

## 9. 交付物与证据

- 提交见 git log（config crate / 度量网格+HiDPI / 回退链+渲染选项 / 钩子门控 / 性能优化）；截图 scripts/e2e/out/phase2a-linux-config.png；e2e 一条命令 sh scripts/e2e/run_all.sh。
