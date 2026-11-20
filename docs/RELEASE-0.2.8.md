# leantty 0.2.8

字体层换成 **crossfont**（与 alacritty 同一个库）：启动时不再出现"缺字形方块"。

## 变化

### 字体加载改为惰性模型

旧的 fontdue 会**整文件解析**字体：CJK 面 ~0.34 s、符号字体各 0.05–0.11 s，只能把回退字体放到
后台线程加载 → 首帧先画占位方块，几百毫秒后才变成真实字形。

crossfont 与 alacritty 同构：Linux 用 fontconfig + FreeType、**Windows 用 DirectWrite**、macOS 用 CoreText，
打开 face 只是 mmap，毫秒级。因此：

- **全部字体（含 CJK 回退、内置符号兜底）在渲染器构造完成时已就绪**，首帧就是真实字形；
- 后台字体线程、逐个并入、并入后重绘那一整套机制删除（少一个线程与一条唤醒路径）；
- Windows 侧不再用 GDI 掩码取字形，彩色 emoji 由 DirectWrite 提供（Linux 侧仍为单色，FreeType 未接 CBDT/COLR）。

### 装饰线几何统一

下划线（实线/双线/波浪/点线/虚线）、删除线、上划线的几何由 `style_line_rects` 统一产出，
CPU 光栅与 GPU 四边形共用同一份数据——0.2.7 只改了 CPU 的线宽与基线，GPU 仍是旧公式，
`gpu_parity` 因此出现 1.39% 字形差异；现在两端逐像素一致。

## 验证

- `cargo test --workspace` 全绿（41 个测试二进制）；`clippy -D warnings` 与 release 构建 0 告警。
- 新增 `crates/render/tests/font_ready.rs`：断言**构造完成即可渲染** CJK 与 emoji（无需任何后台等待）。
- `gpu_parity` 10/10（120×30、200×60、1x/2x，含中文/彩色/下划线/选区/滚动条/光标样式）通过。
- `scripts/gates.sh` 10 步全过（含 e2e）。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu`）：`crossfont` 在 Windows 走 `dwrote`，
  不依赖系统 FreeType；**未在 Windows 实机运行**。

## 升级提示

- 字体相关的配置语义不变（`font.size` 仍是点、`font.fallback` 顺序不变）。
- 若系统缺少 fontconfig/FreeType 运行库（Debian/Devuan 上是 `libfontconfig1`、`libfreetype6`），
  Linux 侧需要安装（deb 依赖已包含）。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.8_amd64.deb` | 见 SHA256SUMS | 见 SHA256SUMS |
| `leantty-0.2.8-windows-x86_64.zip` | 见 SHA256SUMS | 见 SHA256SUMS |
