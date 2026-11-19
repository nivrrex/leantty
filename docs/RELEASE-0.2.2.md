# leantty 0.2.2

修复启动早期中文（以及其它需要回退字体的字符）显示为方块的问题，并缩短方块存在的时间。

## 修复

- **GPU 后端：并入回退字体后仍显示方块。** 字形图集的键是（样式组、码点、像素字号、缩放），
  不含字体索引；字体并入前按"缺字形"缓存下来的方块位图，在并入后仍被同一个键命中，
  于是整个会话都在画方块。现在并入字体时会清空图集（保留同一张纹理，避免重建导致的
  绑定组失效）。
- **启动早期不重绘。** 字体并入后现在会立刻请求重绘，不再等下一次输入把画面"带出来"。

## 变化

- 回退字体在建窗之前就开始后台加载，并省掉一次 CJK 字体深拷贝：方块期从约 **0.65 s** 缩短到
  约 **0.31 s**（参考机：`size = 16`、`Consolas` + `HYQiHei`）。剩余时间受单面 CJK 字体解析
  （约 0.3 s）限制，已在 README「已知限制」中说明。

## 验证

- CPU 与 GPU 后端在字体就绪后截图**逐像素一致**（差异 0）；`scripts/e2e` 的 `case81` 断言为
  三项：两个后端各自在字体就绪前后画面必须变化、且就绪后两者必须一致。
- 完整门禁（`scripts/gates.sh`）10 步通过、e2e 131/131、黄金帧哈希 `507485fb7945fb8f` 未变。
- Windows：`leantty.exe` 由合并后的分支构建，CPU/GPU 截图差异 0.112%（字形光栅化，仅中文行），
  未出现方块或整字缺失；真实 GPU 未验证（仅 WARP/DX12 软件适配器）。

## 安装

### Debian / Devuan（deb）

```sh
sudo apt install ./leantty_0.2.2_amd64.deb
```

依赖：`libvulkan1`（GPU 后端）、`fonts-dejavu-core`（默认等宽字体）。CPU 后端不依赖 Vulkan。

### Windows（zip）

解压后运行 `leantty.exe`；`leantty-portable.cmd` 会用同目录的配置启动。zip 内含
`conpty.dll` 与 `OpenConsole.exe`（随附 ConPTY），缺失时代码会回落到系统 conhost。

## 已知限制

- 启动早期约 0.3 秒内，需要回退字体的字符仍会先显示为方块，随后自动变成正确字形。
- Windows 上 CPU 与 GPU 截图约 1% 像素不同（字形光栅化差异，非布局）。
- Windows 的 DX12 后端只在 WARP 软件适配器上验证过。
- 部分 Ctrl 组合（`Ctrl+;`、`Ctrl+,`、`Ctrl+.`、`Ctrl+'`、`Ctrl+Shift+;`、`Ctrl+I`、`Ctrl+M`）
  在 Linux 下按既有键位表不产生输入。
- 未发送 kitty 键盘协议的按键释放事件。

## 说明

- zip 内的 `CHANGELOG.md`、`README.md`、`README.zh-CN.md` 与仓库 `22be04e` 之前的提交一致，
  发布说明（本文件）额外展开的细节（GPU 图集失效、启动优化）不影响二进制内容。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.2_amd64.deb` | 3,518,280 B | `dc8404ae2ef563537d31c1a945ef0b02c5e8cd63a494475967375859fce93740` |
| `leantty-0.2.2-windows-x86_64.zip` | 5,795,070 B | `d3ae8a3d79eddb4f779bd1a1bf5f2562fe46bdbd79568746a75164bc5e02e5e9` |
| `SHA256SUMS` | — | 上述两行 |

deb 包内含 `/usr/bin/leantty`（sha256 `53886f5c1a825ec5…`）。deb 本身因 `changelog.Debian.gz`
的时间戳不可逐字节复现；同一源码重复构建的包内二进制可复现。

## 升级

- 源码：`git log --oneline phase9a-perf..rel-0.2.2` 可见本版全部提交；`crates/` 相对 0.2.1 的改动
  仅在渲染器的字形缓存与启动期字体加载，配置格式未变。
- 配置无需迁移。
