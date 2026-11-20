# leantty 0.2.6

修两处与 alacritty 观感不一致的问题：桌面启动时首行多出一个空行、以及分数缩放下格高多 1 px。

## 修复

- **桌面启动时首行有空行**。桌面会话（lxpanel/openbox）的环境里没有 `TERM`，子进程的 shell
  退化成 "dumb" 终端：窗口建好后的一次尺寸变化会让它用空格重画提示符，把第一行擦成空白，
  于是内容整体下移一行。现在与其它终端一致——`TERM` 缺失或是 `dumb` 时给子进程兜底成
  `xterm-256color`。
  复现与验证：`TERM` 未设置时 `first_frame` 光标在 `(18,1)`、第 0 行为空；修复后在 `(18,0)`、
  第 0 行就是提示符。

- **格高比 alacritty 多 1 px**。格高此前用 `ceil`，13 pt / 150% 缩放下每格 31 px，而 alacritty 为 30 px，
  30 行累计高出 30 px——所以同一个字号下 leantty 看着更大（字符横向宽度其实一致）。现在改为
  **四舍五入**，与 alacritty 同口径。

## 验证

同一台机器、同一份配置（120×30、13 pt、`Xft.dpi=144`）：

| | 窗口 | 每格 | 首行文字 |
|---|---|---|---|
| alacritty | 1680×900 | 14×30 | 顶格 |
| leantty 0.2.5 | 1680×**930** | 14×**31** | 下移一格（桌面启动）|
| leantty 0.2.6 | **1680×900** | **14×30** | 顶格 |

- `cargo test -p leantty-render / -p leantty-app / -p leantty-vt / -p leantty-pty` 全绿。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu` + `windres` 图标资源），
  只验证 PE 结构、`.rsrc` 段与版本串，**未在 Windows 实机运行**。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.6_amd64.deb` | 见 SHA256SUMS | 见 SHA256SUMS |
| `leantty-0.2.6-windows-x86_64.zip` | 见 SHA256SUMS | 见 SHA256SUMS |
