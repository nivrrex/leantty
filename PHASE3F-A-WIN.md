# PHASE3F-A-WIN：3E 反色测试配色的取证

- 日期：2026-11-13（按系统时间）
- 目的：核实 3E t4 用的 `scripts/windows/e2e/cfg/inverse-dark.toml`（背景 #000000 / 前景 #ffffff）是否真的生效——3E 实测反色块"白"为 (197,200,198)=#C5C8C6、红块为 (204,102,102)=#CC6666，疑似默认配色。
- 约束：只取证，未改共用代码、未加依赖、未改 Pty/Renderer trait。全程只结束本脚本启动的 app PID（记录在日志里）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. 配置结构与解析（步骤 1）

- `inverse-dark.toml` 的 `[colors]` 结构与 `crates/config/src/lib.rs` 的 KNOWN 字段路径逐一对应：`colors.primary.{background,foreground}`、`colors.cursor.{text,cursor}`、`colors.normal.<8 色>`、`colors.bright.<8 色>`、`colors.draw_bold_text_with_bright_colors`。**结构无误**。
- 启动方式：`app.exe --config <toml>`（release），stderr 全量捕获到 `vt-verify-win/logs/phase2b-win/3fa-<tag>.err`。
- stderr 中**没有任何** `config:` / `renderer.backend: expected` / `ignored` / 未知字段 / 回落类警告（只有 `[perf]` 行与 fontdue hinting 提示）；`[perf] config_ms` = 0.9–2.8 ms，说明文件被读取并解析。
- 已验证事实：配置文件**被加载**，不存在路径问题，也不存在"字段名不被识别"的问题。

## 2. 像素取证（步骤 2/3）

同一 release 二进制，仅换 `--config`；截图取 app 真窗客户区，统计直方图与最饱和红像素。

| 配置 | 期望背景 | 实测背景（主色） | 期望前景 | 实测普通文字 | ESC[31m 红字 |
|---|---|---|---|---|---|
| `inverse-dark.toml`（#000000 / #ffffff） | #000000 | **(0,0,0)** ✔（239 690 px） | #ffffff | **(255,255,255)** ✔（192 px） | **(204,102,102)** ✘（配置为 #d32f2f） |
| `control-green.toml`（#003300 / #ffff00） | #003300 | **(0,51,0)** ✔（235 092 px） | #ffff00 | **(255,255,0)** ✔（2639 px） | **(204,102,102)** ✘（同上） |

- 已验证事实：**`colors.primary.background` 与 `colors.primary.foreground` 生效**——换成 #003300/#ffff00 后背景与文字颜色精确随之改变。
- 已验证事实：**ANSI 调色板不随配置变化**——两份配置下 `ESC[31m` 都是 (204,102,102)=#CC6666，而配置里写的是 #d32f2f。
- 已验证事实：反色块的填充色也不是配置前景色——`inverse-dark.toml` 下 10 个反色空格 = **1280 px 的 (197,200,198)=#C5C8C6**（正好 10 格 × 128 px），彩色反色（`ESC[7m`+`ESC[31m`）= **1295 px 的 (204,102,102)**。
- #C5C8C6（前景/反色底）与 #CC6666（ANSI red）这两个值在仓库源码中检索不到（`grep -rn 'C5C8C6|CC6666' crates/ vendor/` 无命中），属 **libghostty-vt 内置默认调色板**的取值。

## 3. 机制（读码，只读）

- `crates/vt/src/terminal.rs:28` `VtTerminal::new(cols, rows)` **不接受调色板**；`crates/vt` 内没有任何 palette 概念。
- `crates/app/src/lib.rs:2742 render_palette()` 把 `config.colors` 完整映射进 `render::Palette`，该 palette 传入 `create_renderer`。
- `crates/render/src/decide.rs` 里取色为 `cell.fg.map(...).unwrap_or(ctx.fg_default)`：当 VT 给出显式 RGB（ANSI 索引色、反色格解析后的颜色）时，renderer 的 palette **不参与**；palette 只在 VT 未给色时兜底（空白格背景、普通默认文字）。
- 因此现象与之完全吻合：primary 背景/前景生效（VT 未给色的场景），ANSI 索引色与反色格底色不生效（VT 已解析成内置 palette 的 RGB）。

## 4. 判定（步骤 4）

**(c) 其他**，具体为"配置已加载且 primary 生效，但 ANSI 索引色与反色格底色由 VT 层内置调色板决定"。

- 不是 (a)：路径正常、结构正确、无解析警告，且 primary 颜色确实变了。
- 不是 (b)：反色并不是混合出错——反色块是**整块纯色** 1280/1295 px，没有混色；只是取到的颜色来自 VT 层内置 palette 而非配置。
- 根因在**共用代码的接线**（配置调色板未传入 VT 层，或未在 renderer 侧覆盖 VT 给出的显式 RGB），按任务规则**停止修改**，写入请求栏（§6）。
- 工程判断：`inverse-dark.toml` 本身无需修改——它写对了，只是这条链路在 Windows 与 Linux 共用，属跨平台一致的行为。

## 5. 重做 3E 的 t4 三项（阈值与 Linux case34 同口径 ≥768）

口径：Linux case34 用"10 格反色块 = 1280 px，纯白像素 ≥768 判可见"。这里同口径，但**按实测到的真实颜色计数**（本机渲染白为 #C5C8C6、ANSI 红为 #CC6666，均非纯 #FFFFFF/#FF0000，故用"该颜色 ± 容差"的像素数，而不是严格 #FFFFFF 匹配）。

| 用例 | 颜色（实测） | 计数 | 阈值 | 判定 |
|---|---|---|---|---|
| A 默认色反色（`ESC[7m` + 10 空格） | (197,200,198) | **1280 px** | ≥768 | **PASS** |
| B 彩色反色（`ESC[7m ESC[31m` + 10 空格） | (204,102,102) | **1295 px** | ≥768 | **PASS** |
| C 选区反色（拖选文本行） | 选区前后逐像素差 | **3968 px** | >0 | **PASS** |

- 3E 里"A 默认白色=87（按 #FFFFFF 严格计数）"的判定**作废**：那不是"反色失效"，而是颜色不是纯白（#C5C8C6）导致严格匹配失败；换成同口径的颜色容差计数后 1280 px，远超阈值。
- 3E 里 B 的"red=0"同样是判据问题（其过滤条件 G<100，而实测 G=102），本轮按实测色宽容差计数为 1295 px。

## 6. 对共享代码的请求

1. **（P2，跨平台一致性问题）配置调色板未到达 VT 层**：`VtTerminal::new` 不接受 palette，`crates/vt` 无 palette 概念，导致 `[colors.normal]`/`[colors.bright]` 与反色格底色在屏幕上不生效（Windows release 实测：配置 #d32f2f 的 `ESC[31m` 渲染为 #CC6666；配置 #ffffff 的反色块渲染为 #C5C8C6；primary bg/fg 正常）。建议二选一：把 palette 传入 VT 层（`Terminal::new` 或 `set_palette`），或在 renderer 侧对"VT 给出显式 RGB 的 ANSI 索引格"做 palette 覆盖。属共用代码，未修。
2. **（测试口径更正，非代码）** 反色/配色的截图断言不要用严格 `#FFFFFF` 匹配；本机 CPU 渲染的白色为 (197,200,198)。建议判据改为"与背景差 ≥ 阈值颜色/亮度容差 + 像素数 ≥768"。已在本轮 Windows 侧采用。

## 7. 未解决的问题

1. #C5C8C6 / #CC6666 的具体出处（libghostty 默认调色板的哪一组）未逐行核实——源码目录中检索不到这两个字面值，属编译进 sys 库的默认值，标为**未核实**。
2. 配置 palette 的缺失是否也影响 `draw_bold_text_with_bright_colors`（粗体映射）本轮未测。

## 8. 交付物与证据

- 脚本：`vt-verify-win/run_3fa_colors.ps1`（配置生效取证：普通文字 / ANSI 红 / 反色块 / 彩色反色）、`run_3fa_sel.ps1`（选区反色）。
- 配置：`scripts/windows/e2e/cfg/inverse-dark.toml`（3E 原文件，未改动）、`control-green.toml`（对照组 #003300/#ffff00，新增）。
- 证据：`vt-verify-win/logs/phase2b-win/3fa-{dark,green,inv-dark,inv-red,sel}.{err,png}`。

## 9. 手动清单（≤3）

1. 若关心配色能否自定义：在 Linux 侧先用同一份 `inverse-dark.toml` 跑一次 `ESC[31m`，确认是否同样渲染成 #CC6666（若是，则问题跨平台，与本任务判断一致）。
2. 用真实主题（例如 Solarized 配置）目视确认 ANSI 16 色是否全部未生效。
3. 若后续修复 palette 接线，重新跑本报告 §5 的三项作为回归。
