# leantty 0.2.4

与重新发布的 0.2.3 **功能相同**。这个版本号的存在意义是让 tag、源码快照与安装包严格对应：
0.2.3 的 tag 指向的是重打包前的源码提交，而资产是重打包后的构建。

## 内容

- 输入法候选窗跟随光标：锚点改用物理像素、X11 下锚在光标格底边（下移一行）、区域宽两格。
- 窗口管理器快捷键不再写进程序：`Alt+Tab` 与 `Super` 组合不转发；窗口失焦时清空修饰键，
  无焦点时丢弃按键。

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
| `leantty_0.2.4_amd64.deb` | 3,519,172 B | `d78d5723d26cde4f9325e9222b1ce69e1fffc5e99781c1e5c6d2bcf2ea6ccd7a` |
| `leantty-0.2.4-windows-x86_64.zip` | 5,800,176 B | `c60b00e9d09f36ad732ea7cddf03d9e7b5e01fe842ad4273bbc58f7e762a6425` |
