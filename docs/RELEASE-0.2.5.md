# leantty 0.2.5

字号换算与 alacritty 对齐，并补齐三类按键此前静默丢失的问题。

## 变化

- **`font.size` 按点（pt）解释**，与 alacritty 一致：像素 = 字号 × 缩放 × 96/72。
  此前该值被直接当像素用，所以同一个数字下 leantty 的字比 alacritty 小约 25%
  （这正是"leantty 要设 18 才和 alacritty 设 16 差不多"的原因）。现在两边用同一个数字即可。
- **格宽改为四舍五入**（原为向上取整）。此前同一字号下每列比 alacritty 宽 1 px，
  80 列窗口实测 1600 px vs 1520 px；现在两者相同（80×24、字号 16、无内边距时窗口均为 1520×912）。

> 升级提示：如果你的配置里 `font.size` 是为了"看起来够大"而调高的，升级后请改回。
> 例如原来写 `size = 18` 视觉上接近 alacritty 的 16，现在把两边都设成 16 即可（或按喜好重调）。

## 修复

- **`Ctrl+Space` 没有输入法时完全不发字节**。`Named(Space)` 此前没有映射到按键枚举，
  编码分支直接跳过；现在与 xterm/alacritty 一致发 `0x00`（tmux/screen 前缀、Emacs `C-@` 依赖它）。
- **`Ctrl+Shift+<字母>` 与 `Ctrl+Shift+<符号>` 静默丢失**。上游编码器在未启用 Kitty 键盘协议时
  仍会为这类组合产出 `CSI u`（例如 `Ctrl+Shift+A` → `ESC[97;6u`），传统应用会把这一串当字符回显。
  现在应用未启用 Kitty 协议时按 xterm 口径回写控制字节：`Ctrl+Shift+A` → `0x01`、
  `Ctrl+Shift+Z` → `0x1a`、`Ctrl+Shift+[` → `0x1b`、`Ctrl+Shift+\` → `0x1c`、`Ctrl+Shift+]` → `0x1d`。
  启用 Kitty 协议的应用仍收到 `CSI u`。
- 顺带修掉两处映射缺口：`Ctrl+Shift+[`/`]`/`\` 在 winit 里上报的是移位后的字符（`{`/`}`/`|`），
  此前映射不到按键枚举，导致该组合静默无输出。

## 验证

- 逐键字节探针（Xvfb，`-e` 作为 PTY 读者读原始字节）：
  `Ctrl+Space → 00`、`Ctrl+Shift+A → 01`、`Ctrl+Shift+[ → 1b`、`Ctrl+A → 01`、`Ctrl+Shift+Z → 1a`。
- 字号与格宽对照（`Xvfb :99`、80×24、padding 0、DejaVu Sans Mono 16）：
  leantty 与 alacritty 窗口尺寸同为 **1520×912**（改前 leantty 为 1600×912，且字号小 25%）。
- 单元测试：`cargo test -p leantty-app --lib` 111 passed；`cargo test -p leantty-vt` 全绿
  （新增 `ctrl_shift_bytes` 锁定上游 `CSI u` 口径与 `Ctrl+Space` 的 `0x00`）。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu` + `windres` 图标资源），
  只验证 PE 结构、`.rsrc` 段与版本串，**未在 Windows 实机运行**。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.5_amd64.deb` | 3,519,740 B | `411b75122de1cf08e253eaadd23974661c2ce28f2297cc032a758982427947e0` |
| `leantty-0.2.5-windows-x86_64.zip` | 5,804,355 B | `5e2f26041310d704fc025e3330437b6b6c4ef69757511b624436bc6bd81a4615` |
