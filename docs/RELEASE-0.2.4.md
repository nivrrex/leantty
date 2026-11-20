# leantty 0.2.4

本版包含三项修复：输入法候选窗定位、窗口管理器快捷键不再写进程序、`Alt+Tab` 残留 Tab。

## 修复

### 输入法候选窗（IME）

候选窗锚点此前有三处与参照实现（alacritty）不一致，导致候选窗盖住正在输入的行、位置不跟随：

- 传给窗口系统的是逻辑坐标，值却是已经乘过缩放因子的物理像素 → HiDPI 下被再乘一次，锚点整体偏移；
- 锚点取光标格**顶边**。X11 的 XIM 只接受一个点、不接受矩形，候选窗正好压住当前行；
- 区域只有一格宽，输入法面板在可用宽度过窄时会把候选窗甩到窗口右下角。

现在：传物理像素；X11 上锚在光标格**底边**（下移一行）；区域宽**两格**；Wayland/Windows 保持
"光标格本身"的矩形语义（其协议支持真实矩形）。是否为 X11 由窗口的 raw handle 判定。
实测（X11 + ibus）：候选窗随光标移动、贴近底部时自动翻到上方，不再遮挡输入行。

### 窗口管理器快捷键

- `Alt+Tab`、`Super` 组合不再被编码后写进程序。此前 `Alt+Tab` 会变成 `ESC` + `Tab`，
  `Super+Space` 因为修饰位被丢弃会退化成裸空格。
- 窗口失焦时清空修饰键状态，无焦点时丢弃按键（切窗口过渡期的按键属于窗口管理器）。

### `Alt+Tab` 残留 Tab

X11 窗口管理器会抓走 `Alt+Tab`：客户端只收到 `Tab` 的 *release*，随后在窗口重新获得焦点后的
**0–13 ms** 内收到一次补投的裸 `Tab`。那一次补投才是残留 Tab 的来源。现在：

- 焦点恢复后 100 ms 内的裸 `Tab` 丢弃；
- `Alt` 刚释放（≤200 ms）后到达的 `Tab` 丢弃。

裸 `Tab`、`Ctrl+Tab` 不受影响。取证方式：`LEANTTY_KEY_TRACE=<path>` 会把 focus / mods / 每个按键
（state、key、mods、text）带毫秒写进文件。

## 安装

```sh
sudo apt install ./leantty_0.2.4_amd64.deb          # Debian / Devuan
```

Windows：解压 zip，运行 `leantty.exe` 或 `leantty-portable.cmd`（内含随附 ConPTY）。

## 验证

- 单元测试：`cargo test -p leantty-app --lib`（109 例）、`cargo test -p leantty-vt --test alt_tab_bytes`（2 例）。
- 逐键字节探针（Xvfb，`-e` 读字节）：`Alt+Tab` 无字节、裸 `Tab` 仍为 `09`、
  "Alt 释放后 68 ms 的 Tab"被吞。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu` + `windres` 图标资源），
  只验证 PE 结构、`.rsrc` 段与版本串，**未在 Windows 实机运行**。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.4_amd64.deb` | 3,523,868 B | `cc5373b3ab4ae1b211e3267c4af6d07ac5d937df1f0f9be18bd3d7937e83223a` |
| `leantty-0.2.4-windows-x86_64.zip` | 5,803,695 B | `9c32993f5e0307b706fffe81ff4717c2a8caccbe92e47d9a00b646ae99cc33ee` |
