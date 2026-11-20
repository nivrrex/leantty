# leantty 0.2.3

修复输入法候选窗（ibus/微软拼音等）的定位：不再盖住正在输入的那一行，并随光标移动。

## 修复

- **候选窗锚点按参照实现（alacritty）重写**，此前有三处不同：
  - 传给 winit 的是逻辑坐标、值却是已经乘过缩放因子的物理像素 → HiDPI 下会被再乘一次，
    锚点整体偏移；
  - 锚点取光标格**顶边**，X11 上候选窗直接盖住当前行；
  - 区域只有一格宽，部分输入法面板在可用宽度过窄时会把候选窗甩到窗口右下角。
- 新实现：传**物理像素**；X11 上把锚点下移**一行**（锚在光标格底边，XIM 只接受点不接受矩形）；
  区域宽度取**两格**；Wayland/Windows 保持"光标格本身"的矩形语义（其输入法协议支持真实矩形）。
  是否为 X11 由窗口的 raw handle 判定，不依赖环境变量猜测。

- **窗口管理器组合键不再写进程序**：`Super` 组合与 `Alt+Tab`（含 Shift）此前会被编码成
  `ESC` + `Tab` 或裸键送进 PTY（`Alt+Tab` 表现为终端里残留一个 Tab / 触发一次补全）。现在这类
  组合在绑定查表之前就被挡掉；窗口失焦时清空修饰键状态，切窗口时按住 Alt 也不会让之后的字符
  带上 `ESC` 前缀。

## 验证

- Linux（X11 + ibus）：在实际桌面会话中确认候选窗随光标跟随、上下自动翻转，不再遮住输入行。
- 单元测试：`ime_area_rect` 4 例（X11/非 X11 的锚点、HiDPI 下 padding 只乘一次、行号饱和加法）。
- `cargo build --release`、`cargo clippy --all-targets -D warnings`：0 告警。
- **Windows 运行时未验证**：本版 `leantty.exe` 在 Linux 上交叉编译（`x86_64-pc-windows-gnu`），
  只做了结构检查（PE x86-64、`.rsrc` 图标资源存在、版本串 `leantty 0.2.3`），没有在 Windows 上运行过。
  请在 Windows 上用微软拼音确认：候选窗随光标、底行时翻到上方不出屏、125%/150% 缩放下无整体偏移。

## 安装

### Debian / Devuan（deb）

```sh
sudo apt install ./leantty_0.2.3_amd64.deb
```

### Windows（zip）

解压后运行 `leantty.exe`，或双击 `leantty-portable.cmd` 用同目录配置启动。zip 内含
`conpty.dll` 与 `OpenConsole.exe`（随附 ConPTY），缺失时回落到系统 conhost。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.3_amd64.deb` | 3,522,352 B | `11dc05aca0df3f5807e9a3f9a0096fc9354ef08a615214c8121c60855dbd20d8` |
| `leantty-0.2.3-windows-x86_64.zip` | 5,800,053 B | `c703db96deb11632cd09551db5f7fecff78976ca04237d2f450f54bbc1753398` |

本版为覆盖发布：资产已替换为包含 `Alt+Tab`/Super 组合键修复后的构建（版本号仍为 0.2.3）。

## 已知限制

- 启动早期约 0.3 秒内仍需回退字形的字符会先显示为方块（0.2.2 起的既有说明）。
- Windows：CPU 与 GPU 截图约 1% 像素差异在字形光栅化；DX12 仅 WARP 上验证过。
- 本版 Windows 二进制为交叉编译产物，未在 Windows 实机运行验证（见上）。
