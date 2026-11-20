# leantty 0.2.4

与重新发布的 0.2.3 **功能相同**。这个版本号的存在意义是让 tag、源码快照与安装包严格对应：
0.2.3 的 tag 指向的是重打包前的源码提交，而资产是重打包后的构建。

## 内容

- 输入法候选窗跟随光标：锚点改用物理像素、X11 下锚在光标格底边（下移一行）、区域宽两格。
- 窗口管理器快捷键不再写进程序：`Alt+Tab` 与 `Super` 组合不转发；窗口失焦时清空修饰键，
  无焦点时丢弃按键。X11 窗口管理器抓走 `Alt+Tab` 后，会在焦点恢复时补投一次裸 `Tab`，
  该补投（焦点恢复后 100 ms 内的裸 Tab）现在被丢弃。X11 窗口管理器抓走 `Alt+Tab` 后会在焦点恢复时补投一次裸 `Tab`，
  该补投（焦点恢复后 ≤100 ms 的裸 Tab）现在被丢弃。

## 安装

```sh
sudo apt install ./leantty_0.2.4_amd64.deb          # Debian / Devuan
```

Windows：解压 zip，运行 `leantty.exe` 或 `leantty-portable.cmd`（内含随附 ConPTY）。

## 验证

- 单元测试：`cargo test -p leantty-app --lib`（101 例，含 WM 组合键 4 例）、
  `cargo test -p leantty-vt --test alt_tab_bytes`（2 例）。
- `cargo build --release`、`cargo clippy -D warnings`：0 告警。
- Windows 二进制为 Linux 上交叉编译（`x86_64-pc-windows-gnu` + `windres` 图标资源），
  只验证 PE 结构、`.rsrc` 段与版本串，**未在 Windows 实机运行**。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.4_amd64.deb` | 3,523,868 B | `cc5373b3ab4ae1b211e3267c4af6d07ac5d937df1f0f9be18bd3d7937e83223a` |
| `leantty-0.2.4-windows-x86_64.zip` | 5,803,695 B | `9c32993f5e0307b706fffe81ff4717c2a8caccbe92e47d9a00b646ae99cc33ee` |
