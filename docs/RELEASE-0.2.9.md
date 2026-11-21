# leantty 0.2.9

修复 0.2.8 引入的一个字体选择缺陷：配置里的 `font.fallback` 实际被绕过。

## 修复

- **逐字符字形不再由 fontconfig 自行挑选**。0.2.8 用"crossfont 能否渲染该字符"来判断某个字体是否含该字，
  但 crossfont 在缺字时会**静默换一个字体**渲染并返回成功（`crossfont/src/ft/mod.rs::face_for_glyph`）。
  于是主字体被判定为"含所有字符"，`font.fallback` 从未被使用，真正的字体由 fontconfig 逐字符决定——
  表现为同一屏里不同字符来自不同字面（字重、字号、笔画粗细不一致）。
- 现在覆盖率按每个 face 的**真实 cmap** 查询（fontdb 负责"家族名 → 字体文件"，ttf-parser 负责查 cmap），
  并只在主字体缺字时**懒加载**字体库——纯 ASCII 启动零额外开销；家族名必须精确匹配，避免落到替代字体。
- 链上没有的字符不再"硬塞给主字体画方块"，而是不绘制（与 0.2.8 的占位策略一致）。

## 验证

- 新增 `crates/render/tests/font_ready.rs::cjk_resolves_to_configured_fallback`：
  `A` → 主字体（索引 0）、`中` → 配置的 fallback（索引 1）、链上没有的字符 → `None`。
- `cargo test --workspace` 42 个测试二进制全绿；clippy（Linux 与 `x86_64-pc-windows-gnu`）0 告警。
- `scripts/gates.sh` 10 步全过、e2e 131/131、GPU 轮 10/10。
- Windows 二进制为 Linux 交叉编译（`x86_64-pc-windows-gnu` + `windres`），**未在 Windows 实机运行**。

## 已知限制（不变）

- 通用族名（`monospace` 等）的观感取决于系统字体；想要确定效果请写具体族名。
- Linux 侧不渲染彩色 emoji（只取 alpha 通道）；Windows 侧由 DirectWrite 提供彩色。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.9_amd64.deb` | 见 SHA256SUMS | 见 SHA256SUMS |
| `leantty-0.2.9-windows-x86_64.zip` | 见 SHA256SUMS | 见 SHA256SUMS |
