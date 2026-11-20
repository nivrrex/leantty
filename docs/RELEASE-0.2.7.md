# leantty 0.2.7

对齐 alacritty 的下划线样式与 emoji 兜底字体。

## 修复

- **下划线样式真的分开画了**。此前 `SGR 4:3`（波浪）直接按单实线画，点线/虚线只是"每 N 像素点一次"：
  - `4:1` 单实线、`4:2` 双线（两条独立线）、`4:3` 波浪（每 2 px 上下交替）、
    `4:4` 点线（1 亮 1 灭）、`4:5` 虚线（2 亮 2 灭）；
  - 线宽与位置按 alacritty 口径：`max(cell_h/16, 1)`，基线 `h - (线宽 + max(h/12,1))`。
- **emoji / 符号不再显示豆腐块**。用户配置的 `font.fallback` 只含 CJK 字体时，`😀 🚀 🦄` 全是空方框；
  现在回退链尾部追加内置兜底 `Symbola`、`DejaVu Sans`、`Noto Sans Symbols2`（系统里不存在的会自动跳过并打印一行提示）。
  它们与用户回退字体一样在后台线程加载，不拖慢首帧。
  **已知限制**：彩色 emoji 字体（CBDT/BGRA，例如 Noto Color Emoji）不支持，只做单色符号对齐。

## 与 alacritty 的尺寸对照（0.2.6 起已一致，本版不变）

同一台机器、同一份配置（120×30、13 pt、`Xft.dpi=144`）：两者窗口均为 **1680×900**、每格 14×30。
（Zig 侧对照清单里的"Rust 小 20%"是在 v0.2.4 上测的，那时 `font.size` 还被当成像素；0.2.5 改成 pt 换算后已消失。）

## 验证

- 新增 `crates/render/tests/underline_styles.rs`：断言四种样式的像素互不相同、双线占 2×线宽行、
  点线/虚线必须有间隔（回退成实线会变红）。
- `cargo test -p leantty-render`（含渲染像素用例）、`cargo test -p leantty-app --lib` 全绿。
- 实机截图：`emoji: 😀 🚀 🦄` 显示为单色符号；`4:1/4:2/4:3/4:4/4:5` 五种样式可辨。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu` + `windres` 图标资源），
  只验证 PE 结构、`.rsrc` 段与版本串，**未在 Windows 实机运行**。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.7_amd64.deb` | 见 SHA256SUMS | 见 SHA256SUMS |
| `leantty-0.2.7-windows-x86_64.zip` | 见 SHA256SUMS | 见 SHA256SUMS |
