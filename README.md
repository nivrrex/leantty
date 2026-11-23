# leantty

轻量终端模拟器：VT 解析与终端状态来自 [libghostty](https://github.com/ghostty-org/ghostty)，
渲染可用 CPU 光栅器或 wgpu（Linux 上 Vulkan，Windows 上 Vulkan/DX12，实验性），PTY 用
`portable-pty`，窗口与输入用 `winit`。

## 特性

- 由 libghostty 提供正确的网格语义：宽字符、组合字符、重排、回滚、备用屏、OSC 0/2 标题、
  OSC 4/104 调色板、括号粘贴。
- 字体用 [crossfont](https://github.com/alacritty/crossfont)（与 alacritty 同一个库）：
  Linux 走 FreeType + fontconfig、Windows 走 DirectWrite。字体在窗口出现前就已加载完成，
  启动时不会出现"字形还没到位、先画方块"的阶段。缺粗体面的字体（很多中文字体只有单字重）
  默认做合成加粗（`font.synthetic_bold`，默认 `true`），否则 `SGR 1` 会与常规字重完全相同。
- 两套渲染后端输出一致：CPU（软件）与 GPU（wgpu），逐像素对拍差异在 1 LSB 内。
- 选区与复制粘贴（X11 PRIMARY/CLIPBOARD、Windows 剪贴板）、滚轮回滚并显示滚动条、
  拖选到窗口边缘时持续自动滚动且不落后于输出。
- 输入路径非阻塞：大段粘贴由独立写线程写出，窗口持续渲染保持响应；超出上限的粘贴整条拒绝，
  不会切在转义序列中间。
- 空闲就是空闲：无内容变化时不设定时器、不做周期性重绘。
- TOML 配置，依赖少，无守护进程，不依赖 systemd。

## 安装

### deb 包

```sh
sudo apt install ./leantty_*.deb
leantty
```

### 压缩包（便携）

解压到任意目录，运行 `leantty`（Windows 上为 `leantty.exe`）。Windows 另见下文注意事项。

### 从源码构建

需要 Rust（edition 2024 工具链）、C 工具链，以及 vendored 的 ghostty 源码。具体环境变量与
固定版本见 [docs/BUILDING.md](docs/BUILDING.md)。

```sh
cargo build --release
./target/release/leantty
```

## 快速开始

```sh
leantty                              # 用默认配置启动
leantty --config-file my-config.toml # 指定配置文件
leantty --help                       # 查看选项
```

## 配置

- 默认位置：Linux 为 `$XDG_CONFIG_HOME/leantty/leantty.toml`（通常是
  `~/.config/leantty/leantty.toml`），Windows 为 `%APPDATA%\leantty\leantty.toml`。
- 环境变量 `LEANTTY_CONFIG` 可覆盖默认位置。
- `--config-file <路径>` 优先级最高。用这种方式指定的路径必须存在且可读，否则打印错误并以
  退出码 1 结束，不会打开窗口。
- 含全部支持键与默认值的模板：[config/leantty.linux.toml](config/leantty.linux.toml)、
  [config/leantty.windows.toml](config/leantty.windows.toml)。复制对应平台的那份到默认位置即可。

未知键会在 stderr 告警并被忽略；取值非法时回落到默认值。

## 命令行选项

| 选项 | 说明 |
|---|---|
| `-h`、`--help` | 打印用法并退出（无图形会话也可用） |
| `-V`、`--version` | 打印版本（`leantty <版本>`）并退出 |
| `--config-file <路径>` | 使用指定的配置文件 |
| `--working-directory <目录>` | 在该目录中启动会话（子程序继承该目录）|
| `-e`、`--command <程序> [参数...]` | 用指定程序替代配置的 shell，其后所有参数原样传给该程序 |

用法错误时退出码为 2，并在 stderr 给出一行原因与 `Try --help`。

示例：

```sh
leantty -e htop                                  # 用指定程序替代 shell
leantty --working-directory ~/src                # 在指定目录中启动
leantty -e sh -c 'make test; read -r _'          # 需要 shell 语法时显式写 shell
leantty -e sh -c 'echo done'                     # 程序退出后窗口随之关闭
```


## 键鼠

| 动作 | 默认绑定 |
|---|---|
| 复制选区 | `Ctrl+Shift+C`、`Ctrl+Insert`、鼠标中键 |
| 粘贴 | `Ctrl+Shift+V`、`Shift+Insert`、鼠标右键 |
| 回滚 / 前滚 | 鼠标滚轮 |
| 选词 / 选行 | 双击 / 三击 |
| 扩展选区 | 拖拽（拖出窗口边缘会持续滚动） |
| 中断当前命令 | `Ctrl+C` |

绑定可在配置文件的 `[[keyboard.bindings]]` / `[[mouse.bindings]]` 中修改（模板里有示例）。

## 渲染后端

- `renderer.backend = "cpu"`（默认）：软件光栅器，不需要 GPU 驱动。
- `renderer.backend = "gpu"`：wgpu 后端。GPU 路径在 AMD 与 NVIDIA 的 Vulkan 环境下测试过；
  Windows 的 DX12 支持已编译进来，但**未在真实硬件上验证**（只用 WARP 软件适配器跑过）。
- `renderer.backend = "auto"`：优先 GPU，没有可用适配器时回落 CPU（Windows 上 `auto` 恒为 CPU）。
- 环境变量：`WGPU_BACKEND`（逗号分隔，如 `vulkan`）、`WGPU_ADAPTER_NAME`（子串匹配）、
  `LEANTTY_GPU_ALLOW_SOFTWARE=1`（允许软件适配器，如 lavapipe）。

## Windows 注意事项

- 随附的 ConPTY 由**必须放在一起部署的两个文件**组成（`conpty.dll` 与 `OpenConsole.exe`，
  与 `leantty.exe` 同目录）。缺一个时 leantty 回落到系统控制台并打印警告。
- 发布包未签名：首次启动会触发 SmartScreen 警告。
- `leantty-portable.cmd` 会用同目录下的配置文件启动便携版。
- 高 DPI 缩放、输入法与剪贴板的行为与 Linux 一致，但请留意下面的已知限制。

- `font.hinting` 仅为兼容保留、当前不生效（crossfont/FreeType 自己决定是否 hinting）；显式开启会在启动时打印一行提示。

- 鼠标选区按流式（阅读顺序）取内容：跨行拖动会取中间整行；单击只清除选区、不留高亮。
  双击选词、三击选行。

## 已知限制

- **Windows 系统 conhost 的既有问题**：`ESC[7m` 可能被改写成显式配色、`OSC 104`（重置调色板）
  不会到达程序、括号粘贴标记与 SGR 鼠标上报可能被控制台输入通道吞掉、折行可能被物化为 CRLF。
  随附官方 ConPTY 可改善多数问题，`OSC 104` 除外。
- **大段粘贴后按 Ctrl+C**：尚未写出的数据会被丢弃，但已进入控制台输入缓冲的文本仍会被 shell
  执行。
- **超过 12 MiB 的粘贴会被整条拒绝**（一个字节都不写），以保证输入路径非阻塞。
- **Windows 的 DX12** 已编译但只在 WARP 软件适配器上验证过。
- Windows 上 CPU 与 GPU 的截图约 1% 像素不同，差异在字形光栅化而非布局；Linux 上未观察到。
- **通用族名的观感随系统变化**：`font.normal = "monospace"` 这类通用名由 fontconfig 解析，
  不同机器可能匹配到完全不同的字体（例如本机匹配到 Noto Sans CJK Mono，行高约为字号的 1.8 倍）。
  想要确定的观感，请写具体族名（`DejaVu Sans Mono`、`Consolas` 等）。
- **Linux 侧不渲染彩色 emoji**：彩色字形（CBDT/COLR）当前只取 alpha 通道，按单色绘制；
  Windows 侧由 DirectWrite 直接提供彩色 emoji。
- 字形度量来自 FreeType/DirectWrite，与 0.2.7 及更早（fontdue）版本相比，同一个 `font.size`
  下的格子高度可能略有不同（0.2.5 起 `font.size` 已按点解释，格高/格宽按四舍五入取值）。

## 故障排查

1. **窗口全黑 / 不渲染**：试 `renderer.backend = "cpu"`；Linux 上确认 `DISPLAY` 或
   `WAYLAND_DISPLAY` 已设置。
2. **中文显示成方块**：启动阶段方块已不存在（字体在首帧前加载完）；若仍看到方块，说明字体链里
   没有任何一个字体含该字符。在 `font.fallback` 里加一个 CJK 字体（Linux 如 `Noto Sans CJK SC`，
   Windows 如 `Microsoft YaHei`）。排查字体是否可用：Linux 用 `fc-list :lang=zh` 看已装的中文字体，
   `fc-match "Noto Sans CJK SC"` 看实际匹配到哪个文件。
3. **`--config-file` 以退出码 1 结束**：路径不存在或不可读，错误信息里带路径。
4. **GPU 后端启动失败**（`backend = "gpu"` 非零退出）：没有可用的 Vulkan/DX12 适配器，
   改用 `"auto"` 或 `"cpu"`。
5. **别的程序能粘贴、这里没反应**：终端里的程序可能自己接管了括号粘贴，检查该程序的处理方式。
6. **在 leantty 里启动 PowerShell 7 时崩溃**：在 leantty 的 shell 里运行 `pwsh` 会立即失败，报
   `System.ArgumentOutOfRangeException: totalWidth ('-17')`，由 `String.PadRight` 抛在
   `UpdatesNotification.ShowUpdateNotification`（PowerShell 自己的更新提示横幅）。规避：启动前把环境变量
   `POWERSHELL_UPDATECHECK=Off`——已在反馈问题的用户机器上确认有效。*限制：是否只在 leantty 中出现尚未确认；
   conhost / Windows Terminal 的对照目前未测。*

- **键盘滚动用 Shift+PageUp / Shift+PageDown**；无修饰的 PageUp / PageDown 转发给程序。
  配置里的绑定要写 winit 的键名（`PageUp`、`PageDown`），不能写 X keysym 名（`Page_Up`）。
- **部分 Ctrl 组合没有输入**（Linux）：Ctrl+Semicolon、Ctrl+Comma、Ctrl+Period、Ctrl+Apostrophe、
  Ctrl+Shift+Semicolon 以及 Ctrl+I / Ctrl+M 不会送到程序，编码器未提供这些键到控制字符的映射。
- **`-e/--command` 的参数必须是合法 UTF-8**；没有"程序退出后保持窗口"的选项。
- **不发送 kitty 键盘协议的按键释放事件**：请求上报 key-up 的程序只能收到按下事件。

## 许可证

MIT，见 [LICENSE](LICENSE)；第三方组件许可见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
