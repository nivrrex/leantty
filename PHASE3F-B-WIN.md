# PHASE3F-B-WIN：阶段 4F Windows 复验交付

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4f-done`；放开门控后的 CPU 测试在 Windows 首次真实运行；调色板与 OSC 改色复验；用新打点复核启动。未做 GPU，未改 Pty/Renderer trait，未加依赖，未改共用代码。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/tags/phase4f-done` | `25f8026663d3bfc33bc9cc2c3bfecf29c14d6a23` |
| `refs/tags/phase4f-done^{}` | **无此行** —— 轻量 tag（`git cat-file -t` = commit），tag 直接指向上述 commit |

- 合并：快进 `b4c8493 → 25f8026`，0 冲突。
- Windows 专属文件 hash 未变：`crates/pty/src/windows.rs` 96416daa、`crates/pty/tests/pty_windows.rs` c3bb4b20、`docs/WIN-NOTES.md` fa753cca、`.cargo/config.toml` 311a5385；合并 diff 中 `scripts/windows/`、`vt-verify-win/`、`crates/pty/` 前缀 0 文件。

## 2. t2 构建与测试

| 项 | 结果 |
|---|---|
| debug 构建 | rc=0，17.1 s |
| release 构建 | rc=0，15.0 s，**零告警** |
| clippy dev / release `--all-targets -D warnings` | 均 rc=0（logs/3fb-clippy-dev.log、3fb-clippy-rel.log） |
| cargo test --workspace | **rc=0，35 个测试目标全绿，0 failed** |
| cargo tree（`--target x86_64-pc-windows-gnu -e normal`） | wgpu 命中 **0**、pollster 命中 **0**；134 行依赖、去重 80 个 name@version |

### 2.1 逐文件用例表（4E 放开门控后的 render 测试首次在 Windows 真实运行）

| 测试目标 | 用例数 | 通过 | SKIP / ignored | 备注 |
|---|---|---|---|---|
| app unittests | 44 | 44 | 0 | |
| app tests/keyboard | 5 | 5 | 0 | |
| app tests/reader | 4 | 4 | 0 | |
| config unittests / tests/config | 0 / 13 | 13 | 0 | |
| pty unittests | 2 | 2 | 0 | |
| pty tests/pty_unix | 0 | — | — | `#![cfg(unix)]` 不适用 |
| pty tests/pty_windows | 21 | 21 | 0 | 14.2 s |
| render unittests | 12 | 12 | 0 | |
| render tests/background_load | 2 | 2 | 0 | **此前被 cfg(unix) 排除，首次真实运行** 7.7 s |
| render tests/baseline_alignment | 1 | 1 | 0 | 同上，7.0 s |
| render tests/bench_frame | 0 | — | **1 ignored** | `#[ignore]` 基准，需手动 `--ignored` |
| render tests/cell_metrics | 1 | 1 | 0 | 此前排除 |
| render tests/fallback_chain | 3 | 3 | 0 | 此前排除，7.8 s |
| render tests/font_load_time | 0 | — | **1 ignored** | 同上，`#[ignore]` |
| render tests/glyph_clip | 1 | 1 | 0 | 此前排除，**47.0 s** |
| render tests/gpu_parity | 0 | — | — | `#![cfg(target_os = "linux")]`，Windows 不适用 |
| render tests/incremental | 3 | 3 | 0 | 55.2 s |
| render tests/inverse_pixels | 4 | 4 | 0 | 此前排除，1.5 s |
| render tests/palette_config_render | 3 | 3 | 0 | 2.8 s |
| render tests/preedit_ghost | 2 | 2 | 0 | 此前排除，2.4 s |
| render tests/render_pixels | 5 | 5 | 0 | 10.4 s |
| render tests/wide_glyph | 4 | 4 | 0 | 此前排除，**35.7 s** |
| vt unittests | 0 | — | — | |
| vt tests/dirty_probe | 1 | 1 | 0 | |
| vt tests/palette_config | 6 | 6 | 0 | |
| vt tests/rows_text | 1 | 1 | 0 | |
| vt tests/scroll_probe | 2 | 2 | 0 | |
| vt tests/v4_grid | 6 | 6 | 0 | |
| vt tests/v8_snapshot | 1 | 1 | 0 | |
| vt tests/wrap_probe | 2 | 2 | 0 | |

- SKIP 情况：**没有运行时 SKIP**（测试内部未走 skip 分支）；只有 2 个 `#[ignore]`（bench_frame、font_load_time，都是手动基准）。
- 字体族名在 Windows 上的匹配（只读枚举 `System.Drawing.FontFamily`）：**DejaVu Sans Mono / Liberation Mono / Noto Sans CJK SC 均已安装**，Consolas 与 Courier New 也在（`C:\Windows\Fonts` 中 consola*.ttf 4 个、cour*.ttf 14 个）。测试日志里 `font_ms` 的 `family='DejaVu Sans Mono'` 是**请求名**且实际解析成功（`used_style=Regular`），因此这些测试在本机不是"回退到别的字体"勉强通过。

## 3. t3 调色板复验

### 3a 两个新增测试（显式运行）

| 命令 | 结果 |
|---|---|
| `cargo test -p vt --test palette_config` | **6 passed / 0 failed**（ansi_basic、ansi_bright、ansi_bg、truecolor/256 不受影响、default_inverse、osc_overrides_then_resets） |
| `cargo test -p render --test palette_config_render` | **3 passed / 0 failed**（indexed_fg_cell_uses_configured_palette_color、inverse_default_cell_uses_configured_colors、bold_with_bright…） |

### 3b 截图像素复验（release，`--config`，取 app 真窗客户区）

`inverse-dark.toml`：primary bg #000000 / fg #ffffff，normal.red **#d32f2f**，normal.white #757575。
`control-green.toml`：primary bg #003300 / fg #ffff00，其余同。

| 观测点 | 3F-a（合并 4F 前） | 本轮（合并 4F 后） | 与配置是否一致 |
|---|---|---|---|
| `ESC[31m` 红字 | (204,102,102) | **(211,47,47)** | ✔ = normal.red #d32f2f（逐通道 ≤0） |
| 彩色反色块（`ESC[7m`+`ESC[31m`，10 空格） | (204,102,102) | **(211,47,47)**（1297 px） | ✔ = normal.red |
| 默认反色块（`ESC[7m`，10 空格） | (197,200,198) | **(117,117,117) = #757575**（1280–1327 px） | ✘ 期望 primary.foreground（#ffffff / 换配置后 #ffff00），实际是 **normal.white** |
| 空白背景 | (0,0,0) | (0,0,0) / 控制组 (0,51,0) | ✔ |
| 普通文字 | (255,255,255) | (255,255,255) / 控制组 (255,255,0) | ✔ |

- **已验证事实**：4F 的调色板接线在 Windows 生效——ANSI 索引色从内置默认 (#CC6666) 变为配置色 (#d32f2f)，彩色反色同步正确。
- **已验证事实（缺陷候选）**：默认色单元格进入反色后，填充色是 **normal[7] white (#757575)**，而不是 primary.foreground；换配置把 primary.foreground 从 #ffffff 改成 #ffff00 后该块仍是 #757575（两次运行 1280 px / 1280 px），因此与 primary 无关。
- **已验证事实（矛盾点）**：vt 层探针（`vt-verify-win/probe_inv`，同一份配色喂 `ESC[7m`+空格）报告 `cell.fg=None / cell.bg=None`；而 render 单测用 `fg=None,bg=None,inverse=true` 的格子渲染时得到的是 primary.foreground。两者与**屏幕实测的 #757575 都不一致**，说明应用侧还有一条未定位的取色路径（例如 ghostty 对空格/默认色的反色表示、或 app 传参差异）。**机制未定位**，标为待验证，已写入请求 18。
- 3F-a 的"反色块 #C5C8C6"已不成立（那是 4F 之前 VT 内置 palette 的值）。

### 3c OSC 改色（app 层截图）

| 尝试 | 序列 | 观测 |
|---|---|---|
| 1 | `OSC 4;1;rgb:aa/bb/cc` + `ESC[31m` | 未出现 #AABBCC；31m 仍为 (211,47,47) |
| 2（改为 Write-Host 发送） | 同上 | 同上 |
| 3（内联命令，同一注入路径） | 同上 | 同上（mostSaturatedRed = 211,47,47，无 #AABBCC） |

- vt 层：`osc_overrides_then_resets_back_to_configured_palette` **通过**（OSC 4 后 31m = #AABBCC、OSC 104 后回到配置色）。
- app 层：三次截图都**未观察到 #AABBCC**，31m 始终等于配置红。无法区分"OSC 序列未送达（注入/控制台处理）"与"送达但未生效"，标 **未核实**，写入请求 19（建议在 app 侧加 OSC 状态观测点）。

## 4. t4 启动复核（release，5 次）

| 打点 | 中位 | 5 次原始值 |
|---|---|---|
| config_ms | 25.6 | 23.5 / 22.4 / 28.8 / 25.6 / 25.6 |
| window_ms | 60.2 | 60.2 / 61.2 / 57.5 / 59.3 / 62.7 |
| surface_ms | 0.9 | 0.9 / 0.8 / 1.5 / 0.9 / 8.5 |
| pty_spawn_ms | 10.3 | 10.3 / 10.1 / 8.2 / 13.7 / 9.5 |
| **cpu_renderer_ready_ms（4E 新增，since_start）** | **465.9** | 561.3 / 446.3 / 465.9 / 463.1 / 479.0 |
| renderer_ready_ms（since_start） | 679 | 741 / 679 / 653 / 647 / 714 |
| first_pty_byte_ms | 793 | 857 / 793 / 762 / 759 / 852 |
| pre_first_submit_ms | 748 | 819 / 748 / 729 / 735 / 794 |
| first_frame_ms | **781** | 846 / 781 / 751 / 750 / 825 |
| bg_fonts_ms | 900 | 991 / 900 / 873 / 860 / 946 |
| fontdb / 4 组字体（示例 run1） | — | 60.2 / 33.7+30.9+31.0+43.2 = 138.8 |

分段（run1 为例，since_start 绝对值）：fontdb 60.2 → 字体 138.8 → **cpu_renderer_ready 561.3** → window 完成后 renderer_ready 741 → pre_first_submit 819 → first_frame 846。

- **未覆盖率不满足"≤30 ms"**：
  - 字体加载结束（≈199）到 `cpu_renderer_ready`（561.3）之间约 **362 ms** 无打点 —— 即 `CpuRenderer::new` 内字体加载之后的部分（字形缓存/图集构建等）。
  - window（60.2，落在 561.3 之后）到 `renderer_ready`（741）之间约 **120 ms** 无打点。
  - 其余段落（renderer_ready→pre_first_submit ≈ 78 ms、pre_first_submit→first_frame ≈ 27 ms）已由打点覆盖。
- 与 3E 对照：3E first_frame≈764、未覆盖≈359；本轮 781（同一量级），新点 `cpu_renderer_ready` 把 3E 那 359 ms 的缺口**定位到渲染器内部、字体加载之后**（≈362 ms），另一段 ≈120 ms 位于窗口创建后到 renderer_ready 之间。合计仍有 >100 ms 未覆盖，写入请求 20。

## 5. 未解决问题

1. 默认色反色块用 normal.white 而不是 primary.foreground（§3b），且与 vt 探针、render 单测三方不一致，机制未定位（待验证）。
2. OSC 4/104 在 app 层三次截图均未见生效，无法区分送达与否（未核实）。
3. 启动仍有 ≈362 ms（渲染器内部）+ ≈120 ms（窗口→renderer_ready）无打点（§4）。
4. 本机 `bench_frame`、`font_load_time` 两个 `#[ignore]` 基准未运行（需手动 `--ignored`）。

## 6. 对共享代码的请求

1. **请求 18（P2，行为不确定）**：反色 + 默认色的取色。实测屏幕反色块 = `normal[7]`（#757575），配置 `primary.foreground` 改动不影响它；而 `vt` 探针报 `fg=None,bg=None`、`render` 单测对同样格子渲染出 primary fg。三处口径不一致，需要 Linux 侧确认 libghostty 对"反色 + 默认色"单元格的表示，并统一 renderer 的默认取色（预期：primary.foreground）。
2. **请求 19（观测增强）**：app 层缺少 OSC 调色板状态的观测点，导致 OSC 4/104 只能靠截图肉眼判断、无法与"未送达"区分；建议在 vt 侧暴露当前调色板/最近一次 OSC 事件的调试日志（仅 debug）。
3. **请求 20（观测增强）**：启动仍有两段无打点——`CpuRenderer::new` 内字体加载之后（≈362 ms）与 window 创建后到 renderer_ready（≈120 ms）。建议在字形缓存/图集构建完成处、以及窗口创建完成后各加一个 `[perf]` 点。

## 7. 交付物与证据

- 脚本：`vt-verify-win/run_3fa_colors.ps1`（新增 `-RawInv/-InvGfx/-Osc4/-Osc104` 模式）、`run_3fb_osc.ps1`、`probe_inv/`（vt 层反色取色探针）。
- 配置：`scripts/windows/e2e/cfg/{inverse-dark,control-green}.toml`（未改动）。
- 日志/截图：`vt-verify-win/logs/{3fb-clippy-dev,3fb-clippy-rel,3fb-test,3fb-tree,3fb-pal-vt,3fb-pal-render,3fb-perf}.log`；`vt-verify-win/logs/phase2b-win/3fa-{4f-dark,4f-inv,4f-invred,4f-invgfx,rawinv-green,osc4c,redcheck}.{png,err}`。
- 未入库：`probe_inv/target/`、`perfdata/`、任何 conpty.dll/OpenConsole.exe。

## 8. 手动清单（≤3）

1. 在 Linux 侧用同一份 `inverse-dark.toml` 目视确认"默认色反色块"是 primary.foreground 还是 normal.white（对应请求 18）。
2. 用真实输入法/真实主题确认 OSC 4 改色在你日常的终端里是否生效（app 层本轮未核实）。
3. 若后续补了启动打点，重新跑 §4 的 5 次采集比对。
