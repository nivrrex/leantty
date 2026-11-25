# leantty 0.2.15

新增配置热加载（`[general] live_config_reload`，默认开），与 alacritty 同名同义。

## 能热换的

| 类别 | 说明 |
|---|---|
| 配色 | `[colors.*]` 全部（主色/16 色/光标/选区/粗体提亮/淡色），改完下一帧即生效 |
| 字体 | `font.size`（改字号会重建渲染器并按当前窗口像素**重排行列**）、族名/样式、`font.fallback`、`synthetic_bold`、`antialiasing`/`subpixel`/`text_gamma` |
| 绑定 | `keyboard.bindings`、`mouse.bindings` |
| 交互 | `selection.save_to_clipboard`、`selection.semantic_escape_chars`、`scrolling.multiplier`、`cursor.style`/`blink`、`dynamic_title` |

## 需要重启（打印提示，不做半截生效）

`window.dimensions`、`scrolling.history`、`terminal.shell`。

## 实现要点

- **不引入依赖、不用轮询定时器**：一个辅助线程每 400 ms 对配置文件做一次 `stat`
  （mtime + 长度），**只有文件真的变了**才通过 `EventLoopProxy` 唤醒主循环。空闲终端仍然
  不排任何定时唤醒（e2e `case50-idle-no-timer-wake` 继续保持零）。
  单独用 mtime 会漏掉"同一秒内等长改写"，所以长度也算进指纹。
- 主循环收到唤醒后在 `about_to_wait` 里重新读文件并 `config::parse`；解析失败（编辑器常"先截断
  再写"）**保留当前配置**，下轮再试，不会把界面刷成默认值。
- 字体变化走 `create_renderer` 重建 + `apply_resize(last_size)` 重排；只改颜色/选项时直接
  `set_palette`/`set_options`（CPU 后端清字形缓存并整屏重绘，GPU 每帧重算顶点色）。
- 语义与 leantty-zig 一致（Zig 侧 0.1.x 起已有该功能）；`live_config_reload = false` 时连 stat 线程都不启动。

## 测试

- 单测 `live_config_reload_default_on_and_configurable`：默认 true、显式 false 生效、类型错误回落并告警。
- e2e `case84-live-reload-applies`：改 `bg` 与 `font.size` 后，窗口背景像素与 dump 里的 `cols` 都必须变化
  （实测 `gray(16)→srgb(32,64,96)`、`cols 40→29`）。
- e2e `case84-live-reload-disabled`：写入 `live_config_reload = false` 后（该次仍应用），再改配置不再生效
  （`reload 次数` 停在 2）。

## 验证

- `cargo nextest run --all`：253 passed / 1 skipped。
- `cargo clippy --all-targets -D warnings`：0 告警；release 构建 0 告警。
- e2e：`PASS=135 FAIL=0`（含新增 2 项），GPU 轮 10/10。首次跑时 `case27`（增量=全量像素对拍）抖了一次，
  属既有 Xvfb 时序问题，重跑即过、判据未改。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.15_amd64.deb` | 3,528,980 B | `aacaf06c16cfdaef0351c43d59c6e98c922751df5129e10a83de3efe72fc9bd9` |
| `leantty-0.2.15-windows-x86_64.zip` | 5,555,905 B | `f26b58410365352386736c642c44c21dd75fa717a869994a24daae6b66c834a3` |

## 未验证

- Windows 实机的文件系统时间戳行为（`ReadDirectoryChangesW` 未使用，走的是同样的 `stat` 轮询）；
- 编辑器"先删后建"（inode 变化）时是否仍能捕获：`stat` 按路径取，删掉期间会读失败并保留旧配置，
  重建后会再次触发（逻辑上成立，未实测）。
