# leantty 0.2.13

修复"字体没有粗体面时，粗体等于常规字重"（粗体丢失）。

## 问题（先在 Linux 上量出来，再改）

选单字重字体（`汉仪旗黑 55S`，`fc-match` 的 `Bold` 与 `Regular` 指向同一个文件，排除"字体其实有粗体"）：

| 字体 | 常规墨迹总量 | 粗体墨迹总量 | 增幅 |
|---|---|---|---|
| 汉仪旗黑（单字重，修复前） | 95064 | 95064 | **0.0%** |
| DejaVu Sans Mono（有真 Bold） | 136740 | 207672 | +51.9% |

单字重字体下 `SGR 1` 与 `SGR 0` 逐字节相同。根因：族内没有 Bold 面时 crossfont/FreeType
把两个请求解析到同一个 face，不会自己做加粗。

## 修法

- `crates/render/src/fontset.rs`：新增 `needs_synthetic_bold()` + `embolden_bitmap()`。
  判断口径是"**该组该索引的 face 与常规组同一索引的 face 是否渲染出同一张位图**"（探测字符
  `M`/`中`/`0`/`a`），比只看家族名更准：主字体有 Bold、回退的中文字体没有时，中文部分也会加粗。
- 加粗方式：位图向右膨胀 `d = round(px/24)`（至少 1px、至多 3px），位图宽度 +d、步进不变。
  与 leantty-zig 的 `FT_Outline_Embolden(em/24)` 同口径；不用 FreeType 轮廓是为了 Windows
  （DirectWrite 路径没有 FreeType）也能用同一套实现。
- 修复后单字重字体增幅 **+39.6%**，与真 Bold 面的 +51.9% 同量级，不偏粗。
- 配置：`[font] synthetic_bold`（默认 `true`；设 `false` 回到 alacritty 的无合成行为）。

## 测试

- `crates/render/tests/synthetic_bold.rs`（4 项）：单字重字体必须变重；`synthetic_bold=false`
  时逐字节回到常规；有真 Bold 面的字体**不得**被二次加粗；"主字体有 Bold + 回退 CJK 无 Bold"
  的中文粗体也要变重。断言用墨迹量（与字体无关），不写死像素。
- `crates/config/tests/config.rs::synthetic_bold_default_on_and_configurable`：默认 true、
  显式 false 生效、类型错误回落并告警。
- 视觉对照（16pt、汉仪旗黑）：`/root/boldshot/compare.png` 上半为开启合成粗体、下半为关闭。

## 局限

- 只加粗不改字宽（等宽字体格宽不变），所以加重方向是水平向右，不侵占前一格。
- 彩色 emoji 走 alpha 通道，同样会被加粗（效果未经实机核对）。
- 未在 Windows/DirectWrite 实机验证观感（逻辑与平台无关，Windows 截图为待办）。
