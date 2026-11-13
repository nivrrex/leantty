# PHASE4F-LINUX：合并 Windows 3F-a / 配置调色板到达 VT / 稳定性长跑

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 Windows | 取 `phase3f-a-win-done-2`（b4c8493 = win 头）；shared 代码**零改动** |
| t2 调色板到达 VT | libghostty 提供完整接口（未改 vendor）；16 色 + 默认 fg/bg/cursor 生效；9 个新测试 + e2e case36（cpu/gpu 均 2734 目标色像素） |
| t3 稳定性长跑 | 15 分钟 × 2 后端：CPU RSS 0.0%、GPU 0.8%，fd/线程恒定，无 panic，`failed=0` |

## 1. t1 合并 Windows（全部 ls-remote 输出）

```
refs/heads/win                      b4c8493dec337ab2fbed222c856adc86732d8310
refs/tags/phase3f-a-win-done        e3bbbdd92829b9d8d3509deab10260aef70f3b15
refs/tags/phase3f-a-win-done^{}     39e4026a5c3b62771cd7f2ab5833e9bdd36d6990
refs/tags/phase3f-a-win-done-2      0982c7ccce57c9cf356789135b5a33647834152a
refs/tags/phase3f-a-win-done-2^{}   b4c8493dec337ab2fbed222c856adc86732d8310
```

- 两个 tag 解引用后为 **39e4026** 与 **b4c8493**；`git merge-base --is-ancestor` 确认
  39e4026 是 b4c8493 的祖先，且 b4c8493 就是 `refs/heads/win` 的头（与任务预期一致）。
- **选择理由**：按"解引用后最新且包含另一个 tag"取 `phase3f-a-win-done-2`。
- merge-base = 14dd0cf，`git merge --no-ff` 无冲突 → **7cd0853**。
- **shared 范围核对**：22 个文件全部落在 `vt-verify-win/`（本轮新增
  `run_3fa_colors.ps1` / `run_3fa_sel.ps1` 探针脚本），**shared 代码零改动**。

## 2. t2 配置调色板到达 VT 层

### 2.1 t2a 接口取证（只读查阅 vendor，未修改任何文件）

| 层 | 接口 | 位置 |
|---|---|---|
| C | `ghostty_terminal_set` + `GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND=11` / `BACKGROUND=12` / `CURSOR=13` / `PALETTE=14` | `vendor/libghostty-rs/crates/libghostty-vt-sys/src/bindings.rs:2834-2846` |
| C 语义 | NULL 清除/重置为内置默认；palette 设置**保留 OSC per-index 覆盖**；16..255 为 xterm 立方/灰阶 | `vendor/ghostty-src/include/ghostty/vt/terminal.h:201-238` |
| Rust | `Terminal::set_default_fg_color` / `set_default_bg_color` / `set_default_cursor_color` / `set_default_color_palette` | `vendor/libghostty-rs/crates/libghostty-vt/src/terminal.rs:1306/1322/1338/1394` |
| Rust 类型 | `Palette(pub [RgbColor;256])`、`Palette::default()` = `ghostty_color_palette_default` | `style.rs:224/240` |

结论：**接口齐备，无需改 vendor、无需新增依赖**。

### 2.2 修复前取证（测试为红）

`crates/vt/tests/palette_config.rs` 用一份"每项都不相同"的调色板初始化终端：

| 序列 | 修复前 | 期望 |
|---|---|---|
| `ESC[30m` | 内置 `Rgb{29,31,33}` | 配置 `{1,2,3}` |
| `ESC[31m` | 内置 `#CC6666`（3F-a 观测） | 配置 `#d32f2f` |
| `ESC[90m..97m`、`ESC[40m..47m`、`ESC[100m..107m` | 全部内置 | 配置 bright / 背景 |
| `ESC[38;2;r;g;b` / `38;5;16/196/232` | 正确（不受影响） | 保持原值 |

### 2.3 修复

- `crates/vt`：新增 `VtPalette { foreground, background, cursor, normal[8], bright[8] }`
  与 `VtTerminal::new_with_palette(cols, rows, &palette)`：
  把配置 16 色覆盖到 libghostty 默认 256 色的**前 16 项**（16..255 保持内置），
  并设默认 fg / bg / cursor。
- `crates/app`：新增 `vt_palette(&config.colors)`（与 `render_palette` 同源字段映射），
  在 `VtTerminal` 构造处传入。

### 2.4 OSC 语义（t2c，已验证）

| 操作 | 结果 |
|---|---|
| `OSC 4;1;rgb:aa/bb/cc` 后 `ESC[31m` | 使用新色 `{0xaa,0xbb,0xcc}` ✅ |
| `OSC 104`（重置全部）后 `ESC[31m` | 回到**配置色** `normal[1]`（不是内置默认）✅ |

即：动态改色与重置仍由 VT 自身处理，配置值成为"默认层"。

### 2.5 默认色反色的表示（影响门禁设计）

libghostty 对 `ESC[7m`（默认色）用 `inverse=true` + `fg/bg=None` 表示，由渲染层用
**配置的** primary fg/bg 交换上色。因此：

- "配置前景 #e0e0e0 作反色底色"在 4F **之前就已正确**（render 层 palette 早已生效）；
- 该断言**无法证伪**本次 VT 层修复，e2e 中仅作回归保护；
- 真正的证伪点是索引色（见 §2.6）。

### 2.6 e2e case36 与证伪

配置：`primary.background=#101010`、`primary.foreground=#e0e0e0`、`normal.red=#d32f2f`；
输出 `ESC[31m` 红字 + `ESC[7m` 默认反色块（10 空格）+ `ESC[7m ESC[31m` 彩色反色块；
截图后用 `-fuzz 2%` 统计整窗目标色像素（容差约每通道 ≤5，理由：X11 截图经
XRender 合成后同一 fill 的 RGB 完全一致，容差只为吸收抗锯齿边缘与采样边界）。

| 版本 | 配置红 `#d32f2f` 像素 | 配置前景 `#e0e0e0` 像素 | 判定 |
|---|---|---|---|
| 回退 VT 接线（修复前） | **0** | 2751 | 红 ✅ |
| HEAD（修复后） | **2734** | 2751 | 绿 ✅ |
| GPU 轮（修复后） | **2734** | — | 绿 ✅ |

阈值 ≥500（10 格 × 8×16 = 1280 的约 40%，比 case34/35 的 60% 更保守，因反色块宽度受
字体与提示符影响）。**未改动 case34/35 的阈值**：它们在默认调色板（primary fg = #ffffff）
下仍然成立，实测全绿。

### 2.7 bold + bright（t2d）

`draw_bold_text_with_bright_colors` 的现有实现与配置语义一致：开启时 bold +
normal 0..7 → bright 8..15，关闭时保持 normal。由
`crates/render/tests/palette_config_render.rs` 的
`bold_with_bright_maps_normal_to_bright_only_when_enabled` 覆盖（实测开启 bright 像素 9、
关闭 bright 像素 0），**无需修复**。

### 2.8 用例与像素验证

- `crates/vt/tests/palette_config.rs`：6 例（ANSI 基本色 / 明亮色 / 背景色、默认色反色
  表示、真彩色与 256 色不受影响、OSC 改色与重置）。
- `crates/render/tests/palette_config_render.rs`：3 例（默认反色块像素 = 配置 fg 作底 +
  配置 bg 作字，实测 74/9；索引前景用配置色；bold+bright 开/关）。
- `gpu_parity` 8 用例仍全过（两侧同用配置调色板，相对比较不受影响）。
- 测试总数 129 → **138**。

## 3. t3 稳定性长跑

`scripts/e2e/soak.sh`（**默认不进 run_all**，`sh scripts/e2e/soak.sh [分钟]`）：

- 负载：每轮 `seq 1 800` 持续滚动；每 6 轮附加一次
  （6000 行爆发 / 随机转义序列：光标移动·颜色·擦除·备用屏·OSC 标题与改色 /
  自身窗口 `windowsize` 缩放 / 1MB 右键粘贴 / 选区拖动 / 滚动条拖动）；
- 采集：每 30s 记录 RSS（`ps -o rss`）、fd 数（`/proc/<pid>/fd`）、线程数
  （`/proc/<pid>/task`），并汇总 `[perf]` 帧日志；
- 收尾：`SIGTERM` 自身 PID 并等待（≤10s），检查 stderr 无 `panicked at` / `RUST_BACKTRACE`。

**结果（release，:99，每后端 15 分钟）**

| 后端 | RSS（预热末 → 峰值） | 增长 | fd | threads | 帧日志 | 判定 |
|---|---|---|---|---|---|---|
| cpu | 214940 → 214940 KB | **0.0%** | 13（恒定） | 3（恒定） | 15 条（≈750 帧） | 通过 |
| gpu | 390312 → 393276 KB | **0.8%** | 54（恒定） | 3（恒定） | 13 条（≈650 帧） | 通过 |

- 两边 stderr 无 panic/backtrace；SIGTERM 后均 10s 内退出；`failed=0`、`rc=0`。
- **观察（非缺陷）**：帧日志偏少是因为 `seq 1 800` 很快完成、每轮间隔 1s，画面多数
  时间静止；RSS/fd/threads 全程稳定且无错误日志，说明不是渲染停滞。若需要"持续满
  负荷"压测，应改为后台常驻输出流（本轮未改，避免长时间占满机器）。

## 4. 验收

- `cargo test --workspace`：**138 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**92/92 PASS**，`GPU_SKIP=0`，GPU 轮 **7/7**
- Windows 依赖树 `wgpu`/`pollster` 命中：**0**（每个 tag 前复核）

## 5. 未解决问题

1. **默认色反色的门禁不可证伪**（已验证事实）：VT 用 `inverse` 标记 + `fg/bg=None`
   表示默认色反色，渲染层用配置色交换——该路径在 4F 之前就正确。若将来 libghostty
   改为显式色输出，e2e 的"配置前景像素"项才会真正具备证伪能力。
2. **`app` 与 `render` 各有一份 palette 映射**（工程判断，置信度低）：`vt_palette()`
   与 `render_palette()` 都从 `config::Colors` 映射，字段重复。合并成一个转换函数可
   减少漂移风险，但会改动 render 的公开类型边界；本轮未做（不顺手重构）。
   **推翻条件**：若新增颜色字段（如 selection 色）时必须同时改两处，届时合并。
3. **长跑未覆盖"持续满负荷"**（已验证事实）：见 §3 的观察。若要验证长时间满负荷下
   的 RSS，需要把负载改成常驻后台输出，并接受机器被占满 15 分钟以上。
4. **GPU 后端 RSS 基线显著高于 CPU**（390MB vs 215MB，已验证事实）：主要是 wgpu
   实例/设备与图集（2048² R8 = 4MB）+ 驱动侧分配；长跑中稳定，不构成泄漏。

## 6. Windows 3F-b 交接

1. **调色板修复同样适用于 Windows**：`VtTerminal::new_with_palette` 与 `vt_palette()`
   都是跨平台代码，无 `cfg` 分支。Windows 侧应能观察到 `ESC[31m` 使用配置的
   `colors.normal.red`（此前是内置 `#CC6666`），默认反色块底色为配置 foreground。
2. **验证方式**：Windows 上运行 `cargo test -p vt --test palette_config` 与
   `cargo test -p render --test palette_config_render`（4E t4 已放开 unix 门控，
   这些文件在 Windows 上会执行）；若字体族名不匹配会打印 `SKIP:`。
3. **OSC 行为一致**：`OSC 4/10/11` 改色与 `OSC 104/110/111` 重置后回到配置色，
   在 Windows 上同样由 libghostty 处理，不依赖平台。
4. **长跑脚本**：`scripts/e2e/soak.sh` 依赖 `xdotool`/`xclip`/`convert` 与 `:99`
   Xvfb，**不适用于 Windows**；Windows 侧如需长跑请用独立脚本（未提供）。

## 7. 手动清单（≤3）

1. office-pc / Windows：确认 `ESC[31m` 等字体现为主题配置的红（而非偏粉的 `#CC6666`），
   以及默认反色块用的是配置的前景/背景色。
2. 若自定义了 `[colors.normal]` / `[colors.bright]`，确认 16 色与主题一致（这是本次修复
   的目标；`colors.primary` 此前已生效）。
3. 长跑结论基于 :99 的 Xvfb；真实 X + 真实 GPU 的 15 分钟长跑未做，若遇到长时间使用
   后的内存增长，请附 `soak.sh` 的 CSV 便于定位。
